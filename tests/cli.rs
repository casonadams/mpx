mod cli {
    mod definition;
    mod e2e;
    mod entry;
    mod fixture;
    #[cfg(unix)]
    mod incompatible;
    mod nodes;
    mod render;
    mod resolve;
    mod windows;
}
