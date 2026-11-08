mod cli {
    mod buffers;
    mod clients;
    mod definition;
    #[cfg(unix)]
    mod e2e;
    mod entry;
    mod fixture;
    mod if_shell;
    #[cfg(unix)]
    mod incompatible;
    mod nodes;
    mod render;
    mod resolve;
    mod windows;
}
