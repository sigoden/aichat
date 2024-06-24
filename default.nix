{ lib, rustPlatform, pkgconfig, bzip2, zstd, ... }:

rustPlatform.buildRustPackage rec {
  pname = "aichat_server";
  version = "0.0.1";

  nativeBuildInputs = [ pkgconfig ];

  cargoLock.lockFile = ./Cargo.lock;
  src = pkgs.lib.cleanSource ./.;

  buildInputs = [ bzip2 zstd ];
}
