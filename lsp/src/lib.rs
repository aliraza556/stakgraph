mod client;
pub mod git;
pub mod language;
mod utils;

pub use client::strip_root;
pub use language::Language;
pub use utils::*;

use anyhow::{anyhow, Context, Result};
use lsp_types::{GotoDefinitionResponse, Hover,Location};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use tracing::{error, info};

pub type CmdAndRes = (Cmd, std::sync::mpsc::Sender<Res>);
pub type CmdReceiver = mpsc::Receiver<CmdAndRes>;
pub type CmdSender = mpsc::Sender<CmdAndRes>;

#[derive(Debug)]
pub struct DidOpen {
    pub file: PathBuf,
    pub text: String,
    pub lang: Language,
}

#[derive(Debug)]
pub enum Cmd {
    DidOpen(DidOpen),
    GotoDefinition(Position),
    GotoImplementations(Position),
    Hover(Position),
    Stop,
}
impl Cmd {
    pub fn send(self, tx: &mpsc::Sender<CmdAndRes>) -> Result<Res> {
        let (res_tx, res_rx) = mpsc::channel();
        tx.send((self, res_tx))?;
        Ok(res_rx.recv()?)
    }
}

#[derive(Debug)]
pub enum Res {
    Opened(String),
    GotoDefinition(Option<Position>),
    GotoImplementations(Option<Position>),
    Hover(Option<String>),
    Stopping,
    Fail(String),
}

#[derive(Debug, Clone)]
pub struct Position {
    pub file: PathBuf,
    pub line: u32,
    pub col: u32,
}
impl Position {
    pub fn new(file: &str, line: u32, col: u32) -> Result<Self> {
        Ok(Self {
            file: file.into(),
            line,
            col,
        })
    }
    fn from_range(path: &str, range: lsp_types::Range, root: &PathBuf) -> Self {
        let fpath = PathBuf::from(path);
        let file = strip_root(&fpath, root);
        Self {
            file: file.into(),
            line: range.start.line,
            col: range.start.character,
        }
    }
    fn from_def(r: GotoDefinitionResponse, root: &PathBuf) -> Option<Self> {
        //
        match r {
            GotoDefinitionResponse::Scalar(loc) => {
                Some(Self::from_range(&loc.uri.path(), loc.range, root))
            }
            GotoDefinitionResponse::Array(locs) => {
                if locs.is_empty() {
                    return None;
                }
                // if there are multiple, filter out mocks and tests
                let locs_no_mocks:Vec<&Location> = locs.iter().filter(|loc| non_mock_location(loc)).collect();
                let theloc = if locs_no_mocks.len()==1 {
                    locs_no_mocks.first().unwrap()
                } else {
                    locs.first().unwrap()
                };
                Some(Self::from_range(&theloc.uri.path(), theloc.range, root))
            }
            GotoDefinitionResponse::Link(links) => {
                if links.is_empty() {
                    return None;
                }
                let link = links.first().unwrap();
                Some(Self::from_range(
                    link.target_uri.path(),
                    link.target_selection_range,
                    root,
                ))
            }
        }
    }
}

fn non_mock_location(loc: &Location) -> bool {
    !loc.uri.path().contains("mock")&&
    !loc.uri.path().contains("test")&&
    !loc.uri.path().contains("spec")&&
    !loc.uri.path().contains("__")
}

impl TryFrom<Hover> for Res {
    type Error = anyhow::Error;
    fn try_from(h: Hover) -> Result<Self, Self::Error> {
        Ok(Res::Hover(Some(match h.contents {
            lsp_types::HoverContents::Scalar(s) => match s {
                lsp_types::MarkedString::String(s) => s,
                lsp_types::MarkedString::LanguageString(ls) => ls.value,
            },
            lsp_types::HoverContents::Array(a) => {
                if a.is_empty() {
                    return Ok(Res::Hover(None));
                }
                match a.first().context(anyhow!("Hover empty array"))? {
                    lsp_types::MarkedString::String(s) => s.clone(),
                    lsp_types::MarkedString::LanguageString(ls) => ls.value.clone(),
                }
            }
            lsp_types::HoverContents::Markup(m) => m.value,
        })))
    }
}

pub fn spawn_analyzer(
    root_dir: &PathBuf,
    lang: &Language,
    cmd_rx: mpsc::Receiver<CmdAndRes>,
) -> Result<()> {
    let lang = lang.clone();
    let root_dir = Path::new(root_dir).canonicalize()?;
    println!("spawning analyzer for {:?} at {:?}", lang, root_dir);

    let _task = tokio::spawn(async move {
        if let Err(e) = spawn_inner(&lang, &root_dir, cmd_rx).await {
            error!("spawn LSP error: {:?}", e);
        }
    });

    // std::thread::sleep(std::time::Duration::from_secs(10));

    Ok(())
}

async fn spawn_inner(lang: &Language, root_dir: &PathBuf, cmd_rx: CmdReceiver) -> Result<()> {
    let executable = lang.lsp_exec();
    info!("child process starting: {}", executable);
    let mut child_config = async_process::Command::new(executable);
    child_config
        .current_dir(&root_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    for a in lang.lsp_args() {
        child_config.arg(a);
    }
    let child = child_config
        .spawn()
        .map_err(|e| anyhow!("spawn error: {:?}", e))?;
    info!("child process started");
    let stdout = child.stdout.context("no stdout")?;
    let stdin = child.stdin.context("no stdin")?;

    info!("start {:?} LSP client", lang);
    let (mut conn, mainloop, indexed_rx) = client::LspClient::new(&root_dir, &lang);

    let mainloop_task = tokio::spawn(async move {
        mainloop.run_buffered(stdout, stdin).await.unwrap();
    });

    let init_ret = conn.init().await?;
    info!("Initialized: {:?}", init_ret.server_info);

    // conn.did_open(&mainrs, &main_text).await?;

    info!("waiting.... {:?}", lang);
    sleep(100).await;
    indexed_rx
        .await
        .map_err(|e| anyhow!("bad indexed rx {:?}", e))?;
    info!("indexed!!! {:?}", lang);

    while let Ok((cmd, res_tx)) = cmd_rx.recv() {
        // debug!("got cmd: {:?}", cmd);
        match conn.handle(cmd).await {
            Ok(res) => {
                if let Res::Stopping = res {
                    break;
                }
                let _ = res_tx.send(res);
            }
            Err(e) => {
                // error!("error handling cmd: {:?}", e);
                let _ = res_tx.send(Res::Fail(e.to_string()));
            }
        }
    }
    // Shutdown.
    sleep(1_000).await;
    conn.stop().await.unwrap();
    mainloop_task.await.unwrap();
    Ok(())
}

async fn sleep(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

// #[cfg(test)]
// mod tests {

//     use super::*;

//     // multi_thread is required!!!
//     #[tokio::test(flavor = "multi_thread")]
//     async fn rusty() -> Result<()> {
//         let root = PathBuf::from("/Users/evanfeenstra/code/sphinx-mobile/stakwork-lambda/lsp");
//         let (tx, rx) = mpsc::channel();
//         spawn_analyzer(&root, &Language::Rust, rx)?;
//         let pos = Position::new("src/lib.rs", 40, 40)?;
//         let res = Cmd::GotoDefinition(pos.clone()).send(&tx)?;
//         println!("RES: {:?}", res);
//         Ok(())
//     }

//     // multi_thread is required!!!
//     #[tokio::test(flavor = "multi_thread")]
//     async fn goy() -> Result<()> {
//         let root = PathBuf::from(
//             "/Users/evanfeenstra/code/sphinx-mobile/stakwork-lambda/ast/examples/sphinx-tribes",
//         );
//         let (tx, rx) = mpsc::channel();
//         spawn_analyzer(&root, &Language::Go, rx)?;
//         let pos = Position::new("main.go", 24, 19)?;
//         println!("TRY!!");
//         let res = Cmd::GotoDefinition(pos.clone()).send(&tx)?;
//         println!("RES: {:?}", res);
//         Ok(())
//     }

//     async fn _sleep(secs: u64) {
//         tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
//     }
// }
