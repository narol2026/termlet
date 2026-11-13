import fs from "fs-extra";
import path from "path";
import { fileURLToPath } from "url";
import svgtofont from "svgtofont";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, "..", "..");
const srcDir = path.join(rootDir, "assets", "icon-font");
const distDir = path.join(rootDir, "fonts", "icons");

// Clean and normalize SVGs: strip hardcoded fill colors so icon fonts
// inherit text color via currentColor.
const cleanedDir = path.join(rootDir, "node_modules", ".tmp-icons");
await fs.ensureDir(cleanedDir);

const iconFiles = (await fs.readdir(srcDir)).filter((f) => f.endsWith(".svg"));

for (const file of iconFiles) {
  const filePath = path.join(srcDir, file);
  let svg = await fs.readFile(filePath, "utf8");

  // Remove hardcoded fill attributes on <path> elements so font color applies.
  // Keep the path data but strip fill="..." and fill="rgb(...)".
  svg = svg.replace(/fill="[^"]*"/gi, "");

  // Remove HTML comments (Font Awesome license)
  svg = svg.replace(/<!--[\s\S]*?-->/g, "");

  await fs.writeFile(path.join(cleanedDir, file), svg, "utf8");
  console.log(`  cleaned: ${file}`);
}

console.log(`\nGenerating font from ${iconFiles.length} SVG icons...\n`);

await svgtofont({
  src: cleanedDir,
  dist: distDir,
  fontName: "my-icons",
  classNamePrefix: "myi",
  startUnicode: 0xea01,
  css: false,
  emptyDist: false,
  website: false,
  generateInfoData: true,
  excludeFormat: ["woff2", "woff", "eot", "svg", "symbol"],
  svgicons2svgfont: {
    fontName: "my-icons",
    fontWeight: 400,
    normalize: true,
    centerHorizontally: true,
    centerVertically: true,
    fixedWidth: true,
    fontHeight: 512,
    descent: 64,
  },
});

// Read the generated info.json to get the unicode mapping
const infoPath = path.join(distDir, "info.json");
if (await fs.pathExists(infoPath)) {
  const info = await fs.readJSON(infoPath);
  console.log("\nGlyph mapping:");
  for (const [name, data] of Object.entries(info)) {
    console.log(`  ${name} -> U+${data.unicode.toUpperCase()}`);
  }
}

// Rename the TTF to a cleaner name
const ttfSrc = path.join(distDir, "my-icons.ttf");
if (await fs.pathExists(ttfSrc)) {
  console.log(`\nFont generated: ${ttfSrc}`);
}

// Cleanup
await fs.remove(cleanedDir);
console.log("Done.");
