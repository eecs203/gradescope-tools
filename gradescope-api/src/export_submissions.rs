use anyhow::{Context, Result};
use async_zip::base::read::stream::ZipFileReader;
use futures::AsyncRead;
use tracing::info;

pub async fn read_zip(zip_data: impl AsyncRead + Unpin) -> Result<()> {
    let mut zip = ZipFileReader::new(zip_data);
    while let Some(zip_reading) = zip
        .next_with_entry()
        .await
        .context("could not read next zip entry")?
    {
        let entry = zip_reading.reader().entry();
        let filename = entry.filename().as_str()?;
        info!(filename, "read file entry from zip");

        zip = zip_reading.skip().await?;
    }

    Ok(())
}
