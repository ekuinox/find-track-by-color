mod client;
mod find;
mod prepare;

use anyhow::{bail, Result};
use clap::Parser;
use client::get_client;
use find::{FindColors, Finder};
use image::Rgb;
use prepare::prepare;
use std::{path::PathBuf, str::FromStr};

#[derive(Clone, Debug)]
pub struct Color(u8, u8, u8);

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
        App::Prepare { directory } => {
            let client = get_client().await?;
            prepare(client, directory).await?;
        }
        App::Find {
            color,
            directory,
            threshold,
            limit,
        } => {
            let finder = FindColors::builder()
                .k(5)
                .runs(1)
                .coverage(0.0025)
                .max_iter(20)
                .verbose(false)
                .seed(0)
                .build()?;
            let client = get_client().await?;
            let finder = Finder::new(threshold, color, limit, directory, finder, client);
            finder.find().await?;
        }
    }
    Ok(())
}
