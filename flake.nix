{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.11";
    nixpkgs-mozilla.url = "github:mozilla/nixpkgs-mozilla/master";
  };

  outputs = { nixpkgs, nixpkgs-mozilla, ... }:
    let
      inherit (nixpkgs) lib;

      makePackage = (system: dev:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ nixpkgs-mozilla.overlays.rust ];
          };

          rust = (pkgs.rustChannelOf {
            channel = "1.75.0";
            sha256 = "sha256-SXRtAuO4IqNOQq+nLbrsDFbVk+3aVA8NNpSZsKlVH/8=";
          }).rust.override {
            extensions = if dev then [ "rust-src" ] else [ ];
          };
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };
        in
        rec {
          web = pkgs.buildNpmPackage {
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
            npmDepsHash = "sha256-gi9Tr2H46oTXYmGC8W/QmhpY4Z+tYAGnNTP86T6OYKA=";
            dontNpmBuild = true;
          };

          default = rustPlatform.buildRustPackage {
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
                "nodejs-bundler-3.0.0" = "sha256-waaz7FkBuhWnz5LMASrwe73Z8KJX2Dq/i9G43Zna98o=";
              };
            };

            nativeBuildInputs = with pkgs; [
              nodejs
              pkg-config
              rustPlatform.bindgenHook
            ] ++ (if dev then
              with pkgs.gst_all_1; [
                gstreamer.bin
              ] else [ ]);

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
        });
    in
    builtins.foldl' lib.recursiveUpdate { } (builtins.map
      (system: {
        devShells.${system} = makePackage system true;
        packages.${system} = makePackage system false;
      }
      )
      lib.systems.flakeExposed);
}
