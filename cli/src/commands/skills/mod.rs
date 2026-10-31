//! `allowit skills`: the paid-service catalog.
mod ls;
mod search;
mod show;

use crate::{
    ctx::Ctx,
    error::Result,
    gateway::{Common, args::Invocation, control},
};
use clap::Subcommand;

pub(crate) const ROUTE: &str = "/api/gateway/services";

/// Search and inspect the paid-service catalog
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Search(search::Args),
    Show(show::Args),
    #[command(visible_alias = "list")]
    Ls(ls::Args),
}

pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    cx.vendor()?.gateway("skills")?;
    match command {
        Command::Search(a) => search::run(a, cx),
        Command::Show(a) => show::run(a, cx),
        Command::Ls(a) => ls::run(a, cx),
    }
}

/// Reads the catalog, optionally narrowed by a query.
pub(crate) fn read(query: Option<&String>) -> Result<serde_json::Value> {
    let inv = Invocation {
        group: "skills".into(),
        args: vec![],
        common: Common::default(),
    };
    let mut text = String::new();
    control::read(&inv, ROUTE, query, &mut text)?;
    serde_json::from_str(&text)
        .map_err(|_| crate::error::Error::uncertain("the catalog reply is not JSON"))
}
