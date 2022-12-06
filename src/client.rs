use anyhow::{Result, bail};
use rspotify::{
    prelude::{BaseClient, OAuthClient},
    scopes, AuthCodePkceSpotify, Config, Credentials, OAuth,
};

pub async fn get_client() -> Result<impl BaseClient + OAuthClient> {
    let Some(creds) = Credentials::from_env() else { bail!("Credentials::from_env failed.") };

    let scopes = scopes!("user-library-read");
    let Some(oauth) = OAuth::from_env(scopes) else { bail!("OAuth::from_env failed.") };
    let mut config = Config::default();
    config.token_cached = true;

    let mut spotify = AuthCodePkceSpotify::with_config(creds, oauth, config);

    let url = spotify.get_authorize_url(None)?;
    spotify.prompt_for_token(&url).await?;

    spotify.write_token_cache().await?;
    Ok(spotify)
}
