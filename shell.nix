with import <nixpkgs> {};
mkShell {
  buildInputs = [
    gdb # required for rust-gdb
    protobuf
    rustup
    rust-analyzer
  ];
}
