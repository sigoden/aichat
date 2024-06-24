{
  description = "aichat_server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ self.overlay ];
        };
      in
      {
        packages.default = pkgs.aichat_server-package;

        overlay = final: prev: {
          aichat_server-package = prev.stdenv.mkDerivation rec {
            pname = "aichat_server";
            version = "1.0.0";

            src = ./.;

            nativeBuildInputs = [ pkgs.cargo pkgs.rustc ];

            buildPhase = ''
              cargo build --release
            '';

            installPhase = ''
              mkdir -p $out/bin
              cp target/release/aichat_server $out/bin/
            '';
          };
        };

        nixosModules = {
          aichat_server-service = { config, lib, pkgs, ... }: {
            options.aichatServer.enable = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Whether to enable the aichat_server-service systemd service.";
            };

            config = lib.mkIf config.aichatServer.enable {
              systemd.services.aichat_server-service = {
                description = "aichat server service";
                after = [ "network.target" ];
                wantedBy = [ "multi-user.target" ];

                serviceConfig = {
                  ExecStart = "${pkgs.aichat_server-package}/bin/aichat_server";
                  Restart = "on-failure";
                };
              };
            };
          };
        };
      });
}
