use std::path::Path;
use std::{io, thread};

use anyhow::{Context as AnyhowContext, Result};
use async_zip::base::read::stream::ZipFileReader;
use futures::channel::mpsc;
use futures::{AsyncBufRead, AsyncRead, SinkExt, Stream, StreamExt, TryStreamExt};
use reqwest::Response;
use tokio::runtime::Handle;
use tracing::{info, trace};

use crate::submission::SubmissionId;
use crate::submission_export::pdf::SubmissionPdf;

pub async fn download_submission_export(response: Response) -> Result<impl AsyncBufRead + Unpin> {
    Ok(response
        .bytes_stream()
        .inspect_ok(|bytes| trace!(num_bytes = bytes.len(), "got byte chunk"))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        .into_async_read())
}

pub fn read_zip(
    zip_data: impl AsyncRead + Unpin + Send + 'static,
) -> impl Stream<Item = Result<(String, Vec<u8>)>> {
    let (sender, receiver) = mpsc::unbounded();
    let handle = Handle::current();

    thread::spawn(move || {
        handle.block_on(async move {
            let send = |result| async { sender.clone().feed(result).await.unwrap() };

            let mut zip = ZipFileReader::new(zip_data);
            loop {
                match zip.next_with_entry().await {
                    Ok(Some(mut zip_reading)) => {
                        let reader = zip_reading.reader_mut();
                        let entry = reader.entry();

                        match entry.filename().as_str() {
                            Err(err) => {
                                send(Err(err).context("cannot decode zip entry filename")).await
                            }
                            Ok(filename) => {
                                if filename.ends_with(".yml") {
                                    info!(filename, "skipping metadata file");
                                } else if !filename.ends_with(".pdf") {
                                    info!(filename, "skipping non-PDF zip entry");
                                } else {
                                    let filename = filename.to_owned();
                                    let mut buf = Vec::new();
                                    let result = reader.read_to_end_checked(&mut buf).await;
                                    match result {
                                        Err(err) => {
                                            send(
                                                Err(err).context("cannot read zip entry file data"),
                                            )
                                            .await
                                        }
                                        Ok(_) => send(Ok((filename, buf))).await,
                                    }
                                }
                            }
                        };

                        let result = zip_reading.skip().await;
                        zip = match result {
                            Ok(zip) => zip,
                            Err(err) => {
                                send(Err(err).context("cannot skip to next zip entry")).await;
                                break;
                            }
                        };
                    }
                    Err(err) => {
                        send(Err(err).context("cannot read next zip entry")).await;
                        break;
                    }
                    Ok(None) => break,
                }
            }
        });
    });

    receiver
}

pub fn files_as_submissions(
    files: impl Stream<Item = Result<(String, Vec<u8>)>>,
) -> impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>> {
    files
        .map(|result| {
            tokio_rayon::spawn(move || {
                let (filename, data) = result?;

                let filename_stem = Path::new(&filename)
                    .file_stem()
                    .context("cannot get PDF filename stem")?
                    .to_str()
                    .expect("the stem should be UTF-8 since all of `filename` is");
                let id = SubmissionId::new(filename_stem.to_owned());

                let submission =
                    SubmissionPdf::new(filename, &data).context("cannot read file as PDF")?;

                anyhow::Ok((id, submission))
            })
        })
        .buffer_unordered(16)
}
