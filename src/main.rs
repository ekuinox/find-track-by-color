use anyhow::{bail, Result};
use clap::Parser;
use futures::{pin_mut, TryStreamExt};
use image::{DynamicImage, GenericImageView, Rgb, Rgba};
use rspotify::{
    model::{FullTrack, Image},
    prelude::{BaseClient, OAuthClient},
    scopes, AuthCodePkceSpotify, Config, Credentials, OAuth,
};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
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
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::try_parse()?;
    match app {
        App::Prepare { directory } => prepare(directory).await?,
        App::Find { color, directory } => {
            dbg!(color, directory);
            todo!()
        }
    }
    Ok(())
}

async fn prepare(directory: PathBuf) -> Result<()> {
    let creds = Credentials::from_env().unwrap();

    let scopes = scopes!("user-library-read");
    dbg!(&scopes);
    let oauth = OAuth::from_env(scopes).unwrap();
    let mut config = Config::default();
    config.token_cached = true;

    let mut spotify = AuthCodePkceSpotify::with_config(creds, oauth, config);

    spotify.write_token_cache().await?;

    let url = spotify.get_authorize_url(None).unwrap();
    spotify.prompt_for_token(&url).await.unwrap();

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
#[allow(unused)]
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
