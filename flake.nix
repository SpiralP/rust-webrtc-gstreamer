{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-22.11";
  };

  outputs = { nixpkgs, ... }:
    let
      inherit (nixpkgs) lib;
    in
    builtins.foldl' lib.recursiveUpdate { } (builtins.map
      (system:
        let
          pkgs = import nixpkgs {
            inherit system;
          };

          inherit (pkgs) rustPlatform buildNpmPackage;

          web = buildNpmPackage {
            name = "webrtc-gstreamer-web";
            src = lib.cleanSourceWith {
              src = ./web;
              filter = path: type:
                let
                  baseName = builtins.baseNameOf (builtins.toString path);
                in
                (
                  baseName == "package.json"
                  || baseName == "package-lock.json"
                );
            };
            npmDepsHash = "sha256-qQ8zEVUXWiYhalRkHjDTOAIrCNNXbl7mQ5T9xdmx53o=";
            dontNpmBuild = true;
          };

          package = rustPlatform.buildRustPackage {
            name = "webrtc-gstreamer";
            src = lib.cleanSourceWith rec {
              src = ./.;
              filter = path: type:
                lib.cleanSourceFilter path type
                && (
                  let
                    baseName = builtins.baseNameOf (builtins.toString path);
                    relPath = lib.removePrefix (builtins.toString ./.) (builtins.toString path);
                  in
                  lib.any (re: builtins.match re relPath != null) [
                    "/Cargo.toml"
                    "/Cargo.lock"
                    "/build.rs"
                    "/src"
                    "/src/.*"
                    "/web"
                    "/web/.*"
                  ]
                );
            };

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "nodejs-bundler-3.0.0" = "sha256-B0Rj8npZ2YM7uh1eW+CSxbE8QD+8jNV8+NuGVSb3BBM=";
              };
            };

            nativeBuildInputs = with pkgs; [
              nodejs
              pkg-config
              rustPlatform.bindgenHook
            ];
            buildInputs = with pkgs; [
              glib
              openssl
            ] ++ (with pkgs.gst_all_1; [
              gstreamer
              gst-plugins-base
              gst-plugins-good
              gst-plugins-bad
              gst-rtsp-server
              gst-editing-services
              gst-libav
              libnice
            ]);


            preConfigure = ''
              ln -s ${web}/lib/node_modules/webrtc-gstreamer-web/node_modules ./web/node_modules
            '';

            doCheck = false;
          };
        in
        rec {
          devShells.${system}.default = package.overrideAttrs (old: {
            nativeBuildInputs = with pkgs; old.nativeBuildInputs ++ [
              clippy
              rustfmt
              rust-analyzer
            ] ++ (with pkgs.gst_all_1; [
              gstreamer.bin
            ]);
          });
          packages.${system} = {
            default = package;
            web = web;
          };
        }
      )
      lib.systems.flakeExposed);
}
