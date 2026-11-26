mod node {
    mod actor;
    mod cwd;
    mod logs;
    mod pacing;
    mod process;
    // Unix only: ConPTY asks for the cursor position before the program runs, and a bare PtyNode
    // has no screen to answer it, so nothing the program writes ever arrives.
    #[cfg(unix)]
    mod pty;
    mod sink;
    mod waits;
}
