mod cli {
    mod buffers;
    mod definition;
    #[cfg(unix)]
    mod e2e;
    mod clients;
    mod entry;
    mod if_shell;
    mod fixture;
    #[cfg(unix)]
    mod incompatible;
    mod nodes;
    mod render;
    mod resolve;
    mod windows;
}
