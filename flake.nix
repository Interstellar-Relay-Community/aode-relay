{
  description = "relay";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
      in
      {
        packages = rec {
          relay = pkgs.callPackage ./relay.nix { };

          default = relay;
        };

        apps = rec {
          dev = flake-utils.lib.mkApp { drv = self.packages.${system}.pict-rs-proxy; };
          default = dev;
        };

        devShell = with pkgs; mkShell {
          nativeBuildInputs = [ cargo cargo-outdated cargo-zigbuild clippy gcc protobuf rust-analyzer rustc rustfmt ];

          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
        };
      });
}
