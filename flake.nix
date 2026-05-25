{
  description = "jscan development and benchmark shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          python = pkgs.python3.withPackages (ps: [
            ps.genson
          ]);
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy

              nodejs
              python
              uv
              go
              cargo-binstall

              jq
              jaq
              ripgrep
              jsongrep

              quicktype
              gron
              fastgron
              duckdb

              hyperfine
              coreutils
            ];

            shellHook = ''
              echo "jscan dev shell: Rust, Node, jq/jaq/rg/jg, quicktype, gron/fastgron, duckdb"
              echo "extra competitors still need bootstrap: jt/jsont, genson-cli, json-to-schema, schemax-cli, drivel"
            '';
          };
        }
      );
    };
}
