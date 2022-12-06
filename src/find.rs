use crate::Color;
use anyhow::{bail, Result};
use image::{DynamicImage, GenericImageView, Rgb, Rgba};
use indicatif::ProgressBar;
use kmeans_colors::{get_kmeans, Kmeans, Sort};
use palette::{IntoColor, Lab, Pixel, Srgb};
use rspotify::{
    model::{FullTrack, TrackId},
    prelude::BaseClient,
};
use std::{
    cmp::Ordering,
    fs::DirEntry,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

#[derive(derive_new::new, Debug)]
pub struct Finder<SPOTIFY: BaseClient> {
    threshold: f64,
    target_color: Color,
    limit: usize,
    directory: PathBuf,
    finder: FindColors,
    spotify: SPOTIFY,
}

impl<SPOTIFY: BaseClient> Finder<SPOTIFY> {
    pub async fn find(self) -> Result<()> {
        let target_color: Rgb<u8> = self.target_color.into();
        let pb = Arc::new(ProgressBar::new(0));
        let finder = Arc::new(self.finder);
        let tasks = std::fs::read_dir(&self.directory)?
            .into_iter()
            .flatten()
            .map(|entry| {
                let pb = pb.clone();
                let finder = finder.clone();
                tokio::spawn(async move {
                    {
                        let r =
                            get_color_by_entry(&finder, &entry).map(|color| (entry.path(), color));
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
            .flat_map(|(path, colors)| {
                let diffs = colors
                    .into_iter()
                    .filter(|(_, per)| *per >= 0.1)
                    .map(|(color, per)| (color_diff(&target_color, &color), per))
                    .collect::<Vec<_>>();
                diffs
                    .into_iter()
                    .find(|(diff, _)| *diff < self.threshold)
                    .map(|(diff, per)| (path, diff, per))
            })
            .flat_map(|(path, diff, per)| {
                track_id_by_image_path(&path).map(|id| (id, path, diff, per))
            })
            .map(|(track_id, path, diff, per)| {
                get_track_with_scores(&self.spotify, track_id.clone(), (track_id, path, diff, per))
            });
        let results = futures::future::join_all(tasks).await;
        let mut tracks = results.into_iter().flatten().collect::<Vec<_>>();
        tracks.sort_by(|(_, (_, _, a, _)), (_, (_, _, b, _))| {
            b.partial_cmp(a).unwrap_or(Ordering::Equal)
        });
        for (track, (id, path, diff, per)) in tracks {
            println!("{} ... {id}, {path:?}, {diff}, {per}", track.name);
        }
        Ok(())
    }
}

async fn get_track_with_scores<S: Sized>(
    spotify: &impl BaseClient,
    track_id: TrackId,
    s: S,
) -> Result<(FullTrack, S)> {
    let track = get_track(spotify, track_id).await?;
    Ok((track, s))
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

fn get_color_by_entry(finder: &FindColors, entry: &DirEntry) -> Result<Vec<(Rgb<u8>, f32)>> {
    let path = entry.path();
    let img = image::open(&path)?;
    let colors = finder.get_colors(img);
    Ok(colors)
}

fn diff(a: u8, b: u8) -> f64 {
    let a = a as f64;
    let b = b as f64;
    let a = a / (u8::MAX as f64);
    let b = b / (u8::MAX as f64);
    a - b
}

fn color_diff(Rgb([a_r, a_g, a_b]): &Rgb<u8>, Rgb([b_r, b_g, b_b]): &Rgb<u8>) -> f64 {
    let d_r = diff(*a_r, *b_r);
    let d_g = diff(*a_g, *b_g);
    let d_b = diff(*a_b, *b_b);
    let x = (d_r.powf(2.0) + d_g.powf(2.0) + d_b.powf(2.0)).sqrt() / 3.0f64.sqrt();
    x.abs()
}

#[derive(derive_builder::Builder, Debug)]
pub struct FindColors {
    runs: usize,
    k: usize,
    max_iter: usize,
    coverage: f32,
    verbose: bool,
    seed: usize,
}

impl FindColors {
    pub fn builder() -> FindColorsBuilder {
        FindColorsBuilder::default()
    }

    fn get_colors(&self, img: DynamicImage) -> Vec<(Rgb<u8>, f32)> {
        let bytes = img
            .pixels()
            .map(|(_, _, Rgba([r, g, b, _]))| [r, g, b])
            .flatten()
            .collect::<Vec<u8>>();
        let lab: Vec<Lab> = Srgb::from_raw_slice(&bytes)
            .iter()
            .map(|x| x.into_format::<f32>().into_color())
            .collect();

        let mut result = Kmeans::new();
        for i in 0..self.runs {
            let run_result = get_kmeans(
                self.k,
                self.max_iter,
                self.coverage,
                self.verbose,
                &lab,
                (self.seed + i) as u64,
            );
            if run_result.score < result.score {
                result = run_result;
            }
        }
        let mut colors = Lab::sort_indexed_colors(&result.centroids, &result.indices)
            .into_iter()
            .map(|color| {
                let per = color.percentage;
                let color: Srgb = color.centroid.into_color();
                let color = color.into_format::<u8>();
                (Rgb([color.red, color.green, color.blue]), per)
            })
            .collect::<Vec<_>>();
        colors.sort_by(|(_, a), (_, b)| b.partial_cmp(&a).unwrap_or(Ordering::Equal));
        colors
    }
}
