{ lib
, stdenv
, fetchFromGitHub
, rustPlatform
, pkgconfig
, bzip2
, zstd
}:

rustPlatform.buildRustPackage rec {
  pname = "aichat_server";
  version = "0.0.1";

  src = ./;


  nativeBuildInputs = [ pkgconfig ];

  cargoLock = {
    lockFile = src + /Cargo.lock;
  };

}
