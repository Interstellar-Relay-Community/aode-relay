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

    #[arg(short, long, help = "Get statistics from the server")]
    stats: bool,

    #[arg(
        short,
        long,
        help = "List domains by when they were last succesfully contacted"
    )]
    contacted: bool,
}

impl Args {
    pub(crate) fn any(&self) -> bool {
        !self.blocks.is_empty()
            || !self.allowed.is_empty()
            || self.list
            || self.stats
            || self.contacted
    }

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

    pub(crate) fn stats(&self) -> bool {
        self.stats
    }

    pub(crate) fn contacted(&self) -> bool {
        self.contacted
    }
}
