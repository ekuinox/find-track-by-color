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
    /// ログインするだけをやる
    #[clap(name = "login")]
    Login,
    /// 保存済みトラック一覧からアルバム画像をわんさかダウンロードする
    #[clap(name = "prepare")]
    Prepare {
        /// 画像を保存するディレクトリ
        #[clap(short = 'd', long = "directory", default_value = "./images")]
        directory: PathBuf,
    },
    /// 色を指定して近いアルバムを見つける
    #[clap(name = "find")]
    Find {
        /// 検索したい色
        color: Color,

        /// 画像を保存したディレクトリ
        #[clap(short = 'd', long = "directory", default_value = "./images")]
        directory: PathBuf,

        /// 色差がこの値以下であれば、ヒットとする
        #[clap(short = 't', long = "threshold", default_value = "0.5")]
        threshold: f64,

        /// 画像ディレクトリから使用するファイル数の上限
        #[clap(short = 'l', long = "limit", default_value = "100")]
        limit: usize,

        /// kmeansのクラスタ数
        #[clap(long = "clusters", default_value = "8")]
        clusters: usize,

        /// kmeansの最大イテレーション数
        #[clap(long = "max-iter", default_value = "20")]
        max_iter: usize,

        /// kmeansのスコアを評価する回数
        #[clap(long = "runs", default_value = "1")]
        runs: usize,

        /// kmeansのシード値
        #[clap(long = "seed", default_value = "0")]
        seed: usize,

        /// kmeansのcoverage ?
        #[clap(long = "coverage", default_value = "0.0025")]
        coverage: f32,

        /// 出力を冗長にするやつ
        #[clap(short = 'v', long = "verbose")]
        verbose: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::try_parse()?;
    match app {
        App::Login => {
            let _client = get_client().await?;
        }
        App::Prepare { directory } => {
            let client = get_client().await?;
            prepare(client, directory).await?;
        }
        App::Find {
            color,
            directory,
            threshold,
            limit,
            clusters,
            max_iter,
            runs,
            coverage,
            seed,
            verbose,
        } => {
            let finder = FindColors::builder()
                .k(clusters)
                .runs(runs)
                .coverage(coverage)
                .max_iter(max_iter)
                .verbose(verbose)
                .seed(seed)
                .build()?;
            let client = get_client().await?;
            let finder = Finder::new(threshold, color, limit, directory, finder, verbose, client);
            finder.find().await?;
        }
    }
    Ok(())
}
