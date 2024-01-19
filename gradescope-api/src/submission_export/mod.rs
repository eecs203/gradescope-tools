use std::path::Path;
use std::{io, thread};

use anyhow::{Context, Result};
use async_zip::base::read::stream::ZipFileReader;
use async_zip::base::read::{WithEntry, ZipEntryReader};
use async_zip::ZipEntry;
use futures::channel::mpsc;
use futures::{AsyncRead, SinkExt, Stream, StreamExt, TryStreamExt};
use tokio::runtime::Handle;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{info, trace};

use self::pdf::{SubmissionPdf, SubmissionPdfStream};

pub mod pdf;

pub struct SubmissionExport<R: AsyncRead + Unpin + Send + 'static> {
    read: R,
}

impl<R: AsyncRead + Unpin + Send + 'static> SubmissionExport<R> {
    pub fn new(read: R) -> Self {
        Self { read }
    }

    pub fn submissions(self) -> impl SubmissionPdfStream {
        self.submission_pdf_bufs()
            .map(|result| {
                tokio_rayon::spawn(move || {
                    let (filename, buf) = result?;
                    Self::pdf_to_submission_pdf(filename, buf)
                })
            })
            .map(|x| x)
            .buffer_unordered(16)
            .map(|x| x)
    }

    fn submission_pdf_bufs(self) -> impl Stream<Item = Result<(String, Vec<u8>)>> {
        let (sender, receiver) = mpsc::unbounded();
        let handle = Handle::current();

        thread::spawn(move || {
            handle.block_on(async move {
                let send = |result| async { sender.clone().feed(result).await.unwrap() };

                let mut zip = ZipFileReader::new(self.read);
                loop {
                    match zip.next_with_entry().await {
                        Ok(Some(mut zip_reading)) => {
                            let reader = zip_reading.reader_mut();

                            if let Some(result) = Self::try_read_pdf_buf(reader).await {
                                send(result).await;
                            }

                            let result = zip_reading.skip().await;
                            zip = match result {
                                Ok(zip) => zip,
                                Err(err) => {
                                    send(Err(err).context("cannot skip to next zip entry")).await;
                                    break;
                                }
                            };
                        }
                        Ok(None) => break,
                        Err(err) => {
                            send(Err(err).context("cannot read next zip entry")).await;
                            break;
                        }
                    }
                }
            });
        });

        receiver
    }

    async fn try_read_pdf_buf(
        reader: &mut ZipEntryReader<'_, impl AsyncRead + Unpin, WithEntry<'_>>,
    ) -> Option<Result<(String, Vec<u8>)>> {
        let entry = reader.entry();
        let filename = match Self::entry_pdf_filename(entry)? {
            Ok(filename) => filename,
            Err(err) => return Some(Err(err)),
        };

        let mut buf = Vec::new();
        let result = reader
            .read_to_end_checked(&mut buf)
            .await
            .context("cannot read zip entry file data");
        if let Err(err) = result {
            return Some(Err(err));
        }

        Some(Ok((filename, buf)))
    }

    fn pdf_to_submission_pdf(filename: String, buf: Vec<u8>) -> Result<SubmissionPdf> {
        let submission_pdf = SubmissionPdf::new(filename, buf)?;
        Ok(submission_pdf)
    }

    fn entry_pdf_filename(entry: &ZipEntry) -> Option<Result<String>> {
        match entry.filename().as_str() {
            Ok(filename) => {
                if filename.ends_with(".pdf") {
                    Some(Ok(filename.to_owned()))
                } else {
                    if filename.ends_with(".yml") {
                        info!(filename, "skipping metadata file");
                    } else {
                        info!(filename, "skipping non-PDF zip entry");
                    }
                    None
                }
            }
            Err(err) => Some(Err(err).context("cannot decode zip entry filename")),
        }
    }
}

pub async fn submissions_export_load(
    path: impl AsRef<Path>,
) -> Result<SubmissionExport<impl AsyncRead + Unpin>> {
    let file = tokio::fs::File::open(path).await?.compat();
    let export = SubmissionExport::new(file);
    Ok(export)
}

pub fn submissions_export_from_response(
    response: reqwest::Response,
) -> SubmissionExport<impl AsyncRead + Unpin> {
    let read = response
        .bytes_stream()
        .inspect_ok(|bytes| trace!(num_bytes = bytes.len(), "got submissions export byte chunk"))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        .into_async_read();
    SubmissionExport::new(read)
}
