// Builds the small EPUB, PDF and CBZ files the mock server serves for the reader (R10).
// Usage (repo root): node scripts/gen-mock-books.mjs   — needs ffmpeg on PATH for the comic pages.
// Output: crates/silo-api/mock-books/ (committed; regenerate only when the fixtures change).
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { crc32 } from "node:zlib";

const out = join("crates", "silo-api", "mock-books");
mkdirSync(out, { recursive: true });

/** A zip with stored (uncompressed) entries, in the given order (EPUB wants `mimetype` first). */
function zip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, data] of entries) {
    const body = Buffer.isBuffer(data) ? data : Buffer.from(data, "utf8");
    const fname = Buffer.from(name, "utf8");
    const crc = crc32(body) >>> 0;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(body.length, 22);
    local.writeUInt16LE(fname.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(body.length, 20);
    central.writeUInt32LE(body.length, 24);
    central.writeUInt16LE(fname.length, 28);
    central.writeUInt32LE(offset, 42);
    locals.push(local, fname, body);
    centrals.push(central, fname);
    offset += local.length + fname.length + body.length;
  }
  const dir = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(dir.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, dir, end]);
}

const tmp = mkdtempSync(join(tmpdir(), "granary-books-"));
function page(label, hue) {
  const file = join(tmp, `${label.replace(/\W+/g, "_")}.jpg`);
  const text = label.replace(/:/g, "\\:");
  execFileSync("ffmpeg", [
    "-v", "error", "-y", "-f", "lavfi",
    "-i", `color=c=0x${hue}:s=600x900`,
    "-vf", `drawtext=fontfile='C\\:/Windows/Fonts/arial.ttf':text='${text}':fontcolor=white:fontsize=44:x=(w-tw)/2:y=(h-th)/2`,
    "-frames:v", "1", "-q:v", "8", file,
  ]);
  return readFileSync(file);
}

// EPUB 3 with a nav document, a stylesheet and an image, three chapters.
const chapter = (n, title, paras) => `<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>${title}</title>
<link rel="stylesheet" href="../css/book.css"/></head><body><section id="c${n}"><h1>${title}</h1>
${paras.map((p) => `<p>${p}</p>`).join("\n")}
</section></body></html>`;
const prose = (seed, count) =>
  Array.from({ length: count }, (_, i) =>
    `${seed} ${i + 1}. The orchard kept its own calendar: blossom, then the long green wait, then the weight of fruit ` +
    `bending every branch towards the grass. Each keeper wrote down what the trees had taught them that year, ` +
    `and left the letters in a tin box under the oldest tree for whoever came next.`,
  );
const epub = zip([
  ["mimetype", "application/epub+zip"],
  ["META-INF/container.xml", `<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>`],
  ["OEBPS/content.opf", `<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">granary-mock-orchard</dc:identifier>
<dc:title>Letters from the Orchard</dc:title><dc:creator>Ada Fenwick</dc:creator><dc:language>en</dc:language></metadata>
<manifest>
<item id="nav" href="text/nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
<item id="css" href="css/book.css" media-type="text/css"/>
<item id="cover" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
<item id="c0" href="text/cover.xhtml" media-type="application/xhtml+xml"/>
<item id="c1" href="text/one.xhtml" media-type="application/xhtml+xml"/>
<item id="c2" href="text/two.xhtml" media-type="application/xhtml+xml"/>
<item id="c3" href="text/three.xhtml" media-type="application/xhtml+xml"/>
</manifest>
<spine><itemref idref="c0"/><itemref idref="c1"/><itemref idref="c2"/><itemref idref="c3"/></spine></package>`],
  ["OEBPS/css/book.css", "h1 { font-variant: small-caps; } p { text-indent: 1.5em; margin: 0 0 .6em; }\n"],
  ["OEBPS/images/cover.jpg", page("Letters from the Orchard", "3b5d3a")],
  ["OEBPS/text/nav.xhtml", `<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Contents</title></head>
<body><nav epub:type="toc"><ol>
<li><a href="cover.xhtml">Cover</a></li>
<li><a href="one.xhtml">Blossom</a></li><li><a href="two.xhtml">The Green Wait</a></li><li><a href="three.xhtml">Windfall</a></li>
</ol></nav></body></html>`],
  ["OEBPS/text/cover.xhtml", `<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Cover</title></head>
<body><img src="../images/cover.jpg" alt="Cover" style="max-width:100%"/><script>document.title = "x";</script></body></html>`],
  ["OEBPS/text/one.xhtml", chapter(1, "Blossom", prose("Spring", 14))],
  ["OEBPS/text/two.xhtml", chapter(2, "The Green Wait", prose("Summer", 18))],
  ["OEBPS/text/three.xhtml", chapter(3, "Windfall", prose("Autumn", 12))],
]);
writeFileSync(join(out, "orchard.epub"), epub);

// Two comic chapters (CBZ), four pages each; names out of order on purpose (the reader sorts naturally).
for (const n of [1, 2]) {
  const pages = [10, 2, 1, 3].map((p) => [`Chapter ${n}/page${p}.jpg`, page(`Chapter ${n} - Page ${p}`, n === 1 ? "6b2d5c" : "2d4f6b")]);
  writeFileSync(join(out, `lanterns-${n}.cbz`), zip([["ComicInfo.xml", "<ComicInfo/>"], ...pages]));
}

// A three-page PDF with one line of text per page.
{
  const objs = [];
  const pageIds = [3, 5, 7];
  objs[1] = "<< /Type /Catalog /Pages 2 0 R >>";
  objs[2] = `<< /Type /Pages /Kids [${pageIds.map((i) => `${i} 0 R`).join(" ")}] /Count 3 >>`;
  pageIds.forEach((id, i) => {
    const text = `BT /F1 28 Tf 72 700 Td (Field notes, page ${i + 1}) Tj ET`;
    objs[id] = `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 9 0 R >> >> /Contents ${id + 1} 0 R >>`;
    objs[id + 1] = `<< /Length ${text.length} >>\nstream\n${text}\nendstream`;
  });
  objs[9] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
  let pdf = "%PDF-1.4\n";
  const offsets = [];
  for (let i = 1; i < objs.length; i++) {
    offsets[i] = Buffer.byteLength(pdf);
    pdf += `${i} 0 obj\n${objs[i]}\nendobj\n`;
  }
  const xref = Buffer.byteLength(pdf);
  pdf += `xref\n0 ${objs.length}\n0000000000 65535 f \n`;
  for (let i = 1; i < objs.length; i++) pdf += `${String(offsets[i]).padStart(10, "0")} 00000 n \n`;
  pdf += `trailer\n<< /Size ${objs.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  writeFileSync(join(out, "field-notes.pdf"), pdf);
}

rmSync(tmp, { recursive: true, force: true });
console.log(`mock books written to ${out}`);
