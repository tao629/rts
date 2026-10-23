use ftail::Ftail;

pub fn new() {
    let mut ftail_builder = Ftail::new();

    #[cfg(debug_assertions)]
    {
        ftail_builder.console(log::LevelFilter::Debug);
    }
}
