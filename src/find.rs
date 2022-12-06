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
    threshold: u8,
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

fn get_color_by_entry(finder: &FindColors, entry: &DirEntry) -> Result<Rgb<u8>> {
    let path = entry.path();
    let img = image::open(&path)?;
    let colors = finder.get_colors(img);
    let Some((color, _)) = colors.first() else { bail!("not found color") };
    Ok(*color)
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

        // Iterate over the runs, keep the best results
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
