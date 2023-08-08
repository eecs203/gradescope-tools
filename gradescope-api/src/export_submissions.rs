use anyhow::Result;
use async_zip::base::read::stream::ZipFileReader;
use futures::AsyncRead;

pub async fn read_zip(zip_data: impl AsyncRead + Unpin) -> Result<()> {
    let mut count = 0;

    let mut zip = ZipFileReader::new(zip_data);
    while let Some(zip_reading) = zip.next_with_entry().await? {
        let entry = zip_reading.reader().entry();
        entry.filename().as_str()?;
        count += 1;
        zip = zip_reading.skip().await?;
    }

    println!("total entries count: {count}");

    Ok(())
}
