use anyhow::{bail, Result};
use clap::Parser;
use futures::{pin_mut, TryStreamExt};
use image::{DynamicImage, GenericImageView, Rgb, Rgba};
use indicatif::ProgressBar;
use rspotify::{
    model::{FullTrack, Image, TrackId},
    prelude::{BaseClient, OAuthClient},
    scopes, AuthCodePkceSpotify, Config, Credentials, OAuth,
};
use std::{
    fs::DirEntry,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};
use tokio::io::{AsyncWriteExt, BufWriter};

#[derive(Clone, Debug)]
struct Color(u8, u8, u8);

impl FromStr for Color {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<css_color::Srgb>() {
            Ok(c) => Ok(Color(
                (c.red * u8::MAX as f32) as u8,
                (c.green * u8::MAX as f32) as u8,
                (c.blue * u8::MAX as f32) as u8,
            )),
            Err(_) => bail!("nanjakore"),
        }
    }
}

impl From<Color> for Rgb<u8> {
    fn from(Color(r, g, b): Color) -> Self {
        Rgb([r, g, b])
    }
}

#[derive(Parser, Debug)]
enum App {
    /// 保存済みトラック一覧からアルバム画像をわんさかダウンロードする
    #[clap(name = "prepare")]
    Prepare {
        #[clap(short = 'd', long = "directory", default_value = "./images")]
        directory: PathBuf,
    },
    /// 色を指定して近いアルバムを見つける
    Find {
        color: Color,
        #[clap(short = 'd', long = "directory", default_value = "./images")]
        directory: PathBuf,
        #[clap(short = 't', long = "threshold", default_value = "10")]
        threshold: u8,
        #[clap(short = 'l', long = "limit", default_value = "10")]
        limit: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::try_parse()?;
    match app {
        App::Prepare { directory } => prepare(directory).await?,
        App::Find {
            color,
            directory,
            threshold,
            limit,
        } => find_first(color, directory, threshold, limit).await?,
    }
    Ok(())
}

async fn find_first(color: Color, directory: PathBuf, threshold: u8, limit: usize) -> Result<()> {
    let creds = Credentials::from_env().unwrap();

    let scopes = scopes!("user-library-read");
    let oauth = OAuth::from_env(scopes).unwrap();
    let mut config = Config::default();
    config.token_cached = true;

    let mut spotify = AuthCodePkceSpotify::with_config(creds, oauth, config);

    let url = spotify.get_authorize_url(None)?;
    spotify.prompt_for_token(&url).await?;

    spotify.write_token_cache().await?;

    let target_color: Rgb<u8> = color.into();
    let pb = Arc::new(ProgressBar::new(0));

    let tasks = std::fs::read_dir(&directory)?
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
        .take(limit)
        .collect::<Vec<_>>();
    pb.set_length(tasks.len() as u64);

    let results = futures::future::join_all(tasks).await;
    let results = results.into_iter().flatten().flatten();

    let tasks = results
        .into_iter()
        .filter(|(path, color)| {
            println!("{path:?}");
            let Rgb(diff) = color_diff(&target_color, color);
            diff.into_iter().all(|c| c < threshold)
        })
        .flat_map(|(path, _)| track_id_by_image_path(&path))
        .map(|track_id| get_track(&spotify, track_id));
    let results = futures::future::join_all(tasks).await;
    let tracks = results.into_iter().flatten();
    for track in tracks {
        println!("{} ... {:?}", track.name, track.preview_url);
    }
    Ok(())
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

async fn prepare(directory: PathBuf) -> Result<()> {
    let Some(creds) = Credentials::from_env() else { bail!("Credentials::from_env failed.") };

    let scopes = scopes!("user-library-read");
    dbg!(&scopes);
    let Some(oauth) = OAuth::from_env(scopes) else { bail!("OAuth::from_env failed.") };
    let mut config = Config::default();
    config.token_cached = true;

    let mut spotify = AuthCodePkceSpotify::with_config(creds, oauth, config);

    spotify.write_token_cache().await?;

    let url = spotify.get_authorize_url(None)?;
    spotify.prompt_for_token(&url).await?;

    let stream = spotify.current_user_saved_tracks(None);
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
