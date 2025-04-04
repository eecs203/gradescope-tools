use std::path::Path;
use std::thread;

use anyhow::{Context, Result};
use async_zip::ZipEntry;
use async_zip::base::read::seek::ZipFileReader;
use async_zip::base::read::{WithEntry, ZipEntryReader};
use async_zip::error::ZipError;
use futures::channel::mpsc;
use futures::{AsyncRead, AsyncSeek, SinkExt, Stream, StreamExt};
use tokio::runtime::Handle;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::info;

use self::pdf::{SubmissionPdf, SubmissionPdfStream};

pub mod pdf;

pub async fn load_submissions_export_from_fs(
    path: impl AsRef<Path>,
) -> Result<impl SubmissionExport> {
    Ok(tokio::fs::File::open(path).await?.compat())
}

pub trait SubmissionExport: AsyncRead + AsyncSeek + Unpin + Send + Sized + 'static {
    fn submissions(self) -> impl SubmissionPdfStream {
        submission_pdf_bufs(self)
            .map(|result| {
                tokio_rayon::spawn(move || {
                    let (filename, buf) = result?;
                    pdf_to_submission_pdf(filename, &buf)
                })
            })
            .map(|x| x)
            .buffer_unordered(16)
            .map(|x| x)
    }
}

impl<R: AsyncRead + AsyncSeek + Unpin + Send + 'static> SubmissionExport for R {}

fn submission_pdf_bufs(
    export: impl SubmissionExport,
) -> impl Stream<Item = Result<(String, Vec<u8>)>> {
    let (sender, receiver) = mpsc::unbounded();
    let handle = Handle::current();

    thread::spawn(move || {
        handle.block_on(async move {
            let send = |result| async { sender.clone().feed(result).await.unwrap() };

            let mut zip = match ZipFileReader::new(export).await {
                Ok(zip) => zip,
                Err(err) => {
                    send(Err(err.into())).await;
                    return;
                }
            };

            let mut index = 0;
            loop {
                match zip.reader_with_entry(index).await {
                    Ok(mut reader) => {
                        index += 1;

                        if let Some(result) = try_read_pdf_buf(&mut reader).await {
                            send(result).await;
                        }
                    }
                    Err(ZipError::EntryIndexOutOfBounds) => break,
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

async fn try_read_pdf_buf<'a>(
    reader: &mut ZipEntryReader<'a, impl SubmissionExport, WithEntry<'a>>,
) -> Option<Result<(String, Vec<u8>)>> {
    let entry = reader.entry();
    let filename = match entry_pdf_filename(entry)? {
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

fn pdf_to_submission_pdf(filename: String, buf: &[u8]) -> Result<SubmissionPdf> {
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
