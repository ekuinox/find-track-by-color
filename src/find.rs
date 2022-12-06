use crate::{Color};
use anyhow::{bail, Result};
use image::{DynamicImage, GenericImageView, Rgb, Rgba};
use indicatif::ProgressBar;
use rspotify::{
    model::{FullTrack, TrackId},
    prelude::BaseClient,
};
use std::{
    fs::DirEntry,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

#[derive(derive_new::new, Debug)]
pub struct Finder<SPOTIFY: BaseClient> {
    threshold: u8,
    target_color: Color,
    limit: usize,
    directory: PathBuf,
    spotify: SPOTIFY,
}

impl<SPOTIFY: BaseClient> Finder<SPOTIFY> {
    pub async fn find(self) -> Result<()> {
        let target_color: Rgb<u8> = self.target_color.into();
        let pb = Arc::new(ProgressBar::new(0));

        let tasks = std::fs::read_dir(&self.directory)?
            .into_iter()
            .flatten()
            .map(|entry| {
                let pb = pb.clone();
                tokio::spawn(async move {
                    {
                        let r = get_color_by_entry(&entry).map(|color| (entry.path(), color));
                        pb.inc(1);
                        r
                    }
                })
            })
            .take(self.limit)
            .collect::<Vec<_>>();
        pb.set_length(tasks.len() as u64);

        let results = futures::future::join_all(tasks).await;
        let results = results.into_iter().flatten().flatten();

        let tasks = results
            .into_iter()
            .filter(|(path, color)| {
                println!("{path:?}");
                let Rgb(diff) = color_diff(&target_color, color);
                diff.into_iter().all(|c| c < self.threshold)
            })
            .flat_map(|(path, _)| track_id_by_image_path(&path))
            .map(|track_id| get_track(&self.spotify, track_id));
        let results = futures::future::join_all(tasks).await;
        let tracks = results.into_iter().flatten();
        for track in tracks {
            println!("{} ... {:?}", track.name, track.preview_url);
        }
        Ok(())
    }
}

async fn get_track(spotify: &impl BaseClient, track_id: TrackId) -> Result<FullTrack> {
    let track = spotify.track(&track_id).await?;
    Ok(track)
}

fn track_id_by_image_path(path: &Path) -> Result<TrackId> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { bail!("file name none") };
    let Some(uri) = name.strip_suffix(".jpg") else { bail!("not matched") };
    let track_id = TrackId::from_str(uri)?;
    Ok(track_id)
}

fn get_color_by_entry(entry: &DirEntry) -> Result<Rgb<u8>> {
    let path = entry.path();
    let img = image::open(&path)?;
    let color = get_one_color_by_image(img);
    Ok(color)
}

fn diff(a: u8, b: u8) -> u8 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

fn color_diff(Rgb([a_r, a_g, a_b]): &Rgb<u8>, Rgb([b_r, b_g, b_b]): &Rgb<u8>) -> Rgb<u8> {
    Rgb([diff(*a_r, *b_r), diff(*a_g, *b_g), diff(*a_b, *b_b)])
}

/// 画像から代表になる色を一つ返す
/// RGBそれぞれの平均をとって、合わせたものを代表としている
/// https://artteknika.hatenablog.com/entry/2019/09/17/151412
/// https://crates.io/crates/kmeans_colors 使えるかも?
fn get_one_color_by_image(img: DynamicImage) -> Rgb<u8> {
    let colors = img
        .pixels()
        .map(|(_, _, color)| color)
        .into_iter()
        .collect::<Vec<_>>();
    let colors_len = colors.len();
    let r = colors
        .iter()
        .fold(0usize, |sum, Rgba(color)| sum + color[0] as usize)
        / colors_len;
    let g = colors
        .iter()
        .fold(0usize, |sum, Rgba(color)| sum + color[1] as usize)
        / colors_len;
    let b = colors
        .iter()
        .fold(0usize, |sum, Rgba(color)| sum + color[2] as usize)
        / colors_len;
    let color = Rgb([r as u8, g as u8, b as u8]);
    color
}
