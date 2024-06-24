{ pkgs, rustPlatform, pkg-config, bzip2, zstd, ... }:

rustPlatform.buildRustPackage {
  pname = "aichat_server";
  version = "0.0.1";

  nativeBuildInputs = [ pkg-config ];

  cargoLock.lockFile = ./Cargo.lock;
  src = pkgs.lib.cleanSource ./.;

  buildInputs = [ bzip2 zstd ];
}
