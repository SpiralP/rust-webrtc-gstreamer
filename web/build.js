const NODE_ENV = process.env.NODE_ENV;

require("esbuild").build({
  entryPoints: ["src/index.ts"],
  outfile: "dist/index.js",
  bundle: true,
  sourcemap: true,
  minify: NODE_ENV === "production",
  platform: "browser",
  target: ["chrome88", "firefox85", "safari14", "edge88"],
  define: {
    "process.env.NODE_ENV": JSON.stringify(NODE_ENV),
    "process.env": JSON.stringify({ NODE_ENV }),
  },
});
