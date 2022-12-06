use anyhow::{bail, Result};
use futures::{pin_mut, TryStreamExt};
use rspotify::{
    model::{FullTrack, Image},
    prelude::{BaseClient, OAuthClient},
};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncWriteExt, BufWriter};

pub async fn prepare(client: impl BaseClient + OAuthClient, directory: PathBuf) -> Result<()> {
    let stream = client.current_user_saved_tracks(None);
    pin_mut!(stream);
    println!("Items (blocking):");

    tokio::fs::create_dir_all(&directory).await?;

    // 並列にやれるようにしたいね
    while let Ok(Some(item)) = stream.try_next().await {
        save_track_image(&directory, &item.track).await?;
    }

    Ok(())
}

/// とりあえず画像を保存しまくる
async fn save_track_image(directory: &Path, track: &FullTrack) -> Result<()> {
    let Some(Image { url, .. }) = track.album.images.first() else { bail!("") };
    let Some(track_id) = &track.id else { bail!("") };
    let bytes = reqwest::get(url).await?.bytes().await?;
    let file =
        tokio::fs::File::create(directory.join(track_id.to_string()).with_extension("jpg")).await?;
    let mut writer = BufWriter::new(file);
    writer.write_all(&bytes).await?;
    Ok(())
}
