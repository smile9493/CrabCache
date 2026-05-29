//! CLI for OAuth login flows (Codex device code, etc.).

use crab_auth::oauth::{Authenticator, LoginOptions, codex::CodexAuthenticator};
use crab_auth::store::{FileTokenStore, TokenStore};
use std::env;
use std::path::PathBuf;
use std::process;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let codex_device = args.iter().any(|a| a == "--codex-device-login");
    let codex_login = args.iter().any(|a| a == "--codex-login") || codex_device;

    if !codex_login {
        eprintln!(
            "Usage: crab-oauth --codex-login [--codex-device-login] [--auth-dir DIR] [--no-browser]"
        );
        process::exit(1);
    }

    let auth_dir = args
        .windows(2)
        .find_map(|w| (w[0] == "--auth-dir").then_some(w[1].clone()))
        .map(PathBuf::from)
        .unwrap_or_else(default_auth_dir);

    let no_browser = args.iter().any(|a| a == "--no-browser");

    let authenticator = CodexAuthenticator::new();
    let opts = LoginOptions {
        no_browser,
        device_mode: codex_device,
        ..LoginOptions::default()
    };

    match authenticator.login(&opts).await {
        Ok(record) => {
            let store = FileTokenStore::new(auth_dir);
            match store.save(&record).await {
                Ok(path) => {
                    println!("Authentication saved to {path}");
                    if codex_device {
                        println!("Codex device authentication successful!");
                    } else {
                        println!("Codex authentication successful!");
                    }
                }
                Err(err) => {
                    eprintln!("Failed to save credentials: {err}");
                    process::exit(1);
                }
            }
        }
        Err(err) => {
            eprintln!("Codex authentication failed: {err}");
            process::exit(1);
        }
    }
}

fn default_auth_dir() -> PathBuf {
    env::var("CRABCACHE_AUTH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_home().join(".crabcache").join("auths")
        })
}

fn dirs_home() -> PathBuf {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}
