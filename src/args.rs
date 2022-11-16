use clap::Parser;

#[derive(Debug, Parser)]
#[structopt(name = "relay", about = "An activitypub relay")]
pub(crate) struct Args {
    #[arg(short, help = "A list of domains that should be blocked")]
    blocks: Vec<String>,

    #[arg(short, help = "A list of domains that should be allowed")]
    allowed: Vec<String>,

    #[arg(short, long, help = "Undo allowing or blocking domains")]
    undo: bool,

    #[arg(short, long, help = "List allowed and blocked domains")]
    list: bool,
}

impl Args {
    pub(crate) fn new() -> Self {
        Self::parse()
    }

    pub(crate) fn blocks(&self) -> &[String] {
        &self.blocks
    }

    pub(crate) fn allowed(&self) -> &[String] {
        &self.allowed
    }

    pub(crate) fn undo(&self) -> bool {
        self.undo
    }

    pub(crate) fn list(&self) -> bool {
        self.list
    }
}
