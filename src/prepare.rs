use anyhow::{bail, Result};
use futures::{pin_mut, StreamExt};
use indicatif::ProgressBar;
use rspotify::{
    model::{FullTrack, Image},
    prelude::{BaseClient, OAuthClient},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::{AsyncWriteExt, BufWriter};

pub async fn prepare(client: impl BaseClient + OAuthClient, directory: PathBuf) -> Result<()> {
    let stream = client.current_user_saved_tracks(None);
    pin_mut!(stream);

    tokio::fs::create_dir_all(&directory).await?;

    let items = stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let pb = Arc::new(ProgressBar::new(items.len() as u64));

    let _ = futures::future::join_all(
        items
            .into_iter()
            .map(|item| save_track_image_with_pb(&directory, item.track, pb.clone())),
    )
    .await;

    Ok(())
}

async fn save_track_image_with_pb(
    directory: &Path,
    track: FullTrack,
    pb: Arc<ProgressBar>,
) -> Result<()> {
    let r = save_track_image(&directory, track).await;
    pb.inc(1);
    r?;
    Ok(())
}

/// とりあえず画像を保存しまくる
async fn save_track_image(directory: &Path, track: FullTrack) -> Result<()> {
    let Some(Image { url, .. }) = track.album.images.first() else { bail!("") };
    let Some(track_id) = &track.id else { bail!("") };
    let bytes = reqwest::get(url).await?.bytes().await?;
    let file =
        tokio::fs::File::create(directory.join(track_id.to_string()).with_extension("jpg")).await?;
    let mut writer = BufWriter::new(file);
    writer.write_all(&bytes).await?;
    Ok(())
}
