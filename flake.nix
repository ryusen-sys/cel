{
  description = "Ryusen's CEL implementation and compatibility tooling";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ] (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rust = pkgs.rust-bin.stable."1.98.0".default.override {
          extensions = [
            "rustfmt"
            "clippy"
            "rust-src"
          ];
        };
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rust;
          rustc = rust;
        };
        library = rustPlatform.buildRustPackage {
          pname = "ryusen-cel";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
          src = pkgs.lib.cleanSource self;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "--workspace"
            "--exclude"
            "cel-core-conformance"
          ];
          cargoTestFlags = [
            "--workspace"
            "--exclude"
            "cel-core-conformance"
          ];
        };
      in
      {
        packages.default = library;
        checks.library = library;
        formatter = pkgs.nixfmt;
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rust
            git
            jujutsu
            git-cliff
            gh
            gnumake
            jq
            python3
            actionlint
            shellcheck
            nixfmt
            buf
            protoc-gen-prost
            protoc-gen-prost-crate
          ];
        };
      }
    );
}
