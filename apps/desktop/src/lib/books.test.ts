import { describe, expect, it } from "vitest";
import {
  comicPages,
  containerRootfile,
  eventTime,
  fractionLocation,
  parseNav,
  parseNcx,
  parseOpf,
  readerFile,
  resolveHref,
  restoreFraction,
  sanitizeChapter,
  sanitizeCss,
} from "./books";
import type { FileVersion } from "./catalog";

const file = (file_id: string, container: string) => ({ file_id, container }) as FileVersion;

describe("books", () => {
  it("prefers EPUB, then PDF, then comics, unless a file is asked for", () => {
    const v = [file("1", "cbz"), file("2", "pdf"), file("3", "epub")];
    expect(readerFile(v)?.file_id).toBe("3");
    expect(readerFile(v.slice(0, 2))?.file_id).toBe("2");
    expect(readerFile(v, "1")?.file_id).toBe("1");
    expect(readerFile([file("9", "mobi")])?.file_id).toBe("9");
    expect(readerFile([])).toBeUndefined();
  });

  it("stores and restores fractions, falling back to the progress value", () => {
    expect(fractionLocation(0.25)).toBe("fraction:0.250000");
    expect(fractionLocation(3)).toBe("fraction:1.000000");
    expect(restoreFraction({ location: "fraction:0.4", progress: 0.1 })).toBe(0.4);
    expect(restoreFraction({ location: "epubcfi(/6/2)", progress: 0.25 })).toBe(0.25);
    expect(restoreFraction(null)).toBe(0);
  });

  it("writes event times in UTC milliseconds, slightly behind the clock", () => {
    expect(eventTime(Date.UTC(2026, 0, 2, 3, 4, 8))).toBe("2026-01-02T03:04:05.000Z");
  });

  it("orders comic pages naturally and skips non-images", () => {
    expect(
      comicPages(["c/page10.jpg", "ComicInfo.xml", "c/page2.JPG", "__MACOSX/c/._page1.jpg", "c/page1.png"]),
    ).toEqual(["c/page1.png", "c/page2.JPG", "c/page10.jpg"]);
  });

  it("resolves archive paths and refuses to leave the book", () => {
    expect(resolveHref("OEBPS/text/one.xhtml", "../images/a%20b.jpg#x")).toBe("OEBPS/images/a b.jpg");
    expect(resolveHref("OEBPS/content.opf", "text/one.xhtml")).toBe("OEBPS/text/one.xhtml");
    expect(resolveHref("a.xhtml", "../../x.jpg")).toBeNull();
    expect(resolveHref("a.xhtml", "https://example.com/x")).toBeNull();
    expect(resolveHref("a.xhtml", "#note")).toBeNull();
  });

  it("reads the container, package, nav and NCX documents", () => {
    expect(
      containerRootfile(
        `<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>`,
      ),
    ).toBe("OEBPS/content.opf");
    const opf = parseOpf(
      `<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>T</dc:title></metadata>
      <manifest><item id="nav" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/>
      <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
      <item id="a" href="text/a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="text/b.xhtml" media-type="application/xhtml+xml"/></manifest>
      <spine toc="ncx"><itemref idref="a"/><itemref idref="x"/><itemref idref="b" linear="no"/></spine></package>`,
      "OEBPS/content.opf",
    );
    expect(opf).toEqual({
      title: "T",
      spine: [{ path: "OEBPS/text/a.xhtml", weight: 1 }],
      navPath: "OEBPS/nav.xhtml",
      ncxPath: "OEBPS/toc.ncx",
    });
    const nav = parseNav(
      `<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol>
      <li><a href="text/a.xhtml">One</a><ol><li><a href="text/a.xhtml#s">One.1</a></li></ol></li></ol></nav></body></html>`,
      "OEBPS/nav.xhtml",
    );
    expect(nav).toEqual([
      { label: "One", path: "OEBPS/text/a.xhtml", depth: 0 },
      { label: "One.1", path: "OEBPS/text/a.xhtml", depth: 1 },
    ]);
    const ncx = parseNcx(
      `<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint><navLabel><text>One</text></navLabel><content src="a.xhtml"/>
      <navPoint><navLabel><text>Two</text></navLabel><content src="b.xhtml"/></navPoint></navPoint></navMap></ncx>`,
      "OEBPS/toc.ncx",
    );
    expect(ncx).toEqual([
      { label: "One", path: "OEBPS/a.xhtml", depth: 0 },
      { label: "Two", path: "OEBPS/b.xhtml", depth: 1 },
    ]);
  });

  it("strips active content from chapters and collects their resources", () => {
    const { doc, res } = sanitizeChapter(
      `<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../css/a.css"/><script>alert(1)</script></head>
      <body onload="x()"><img src="../img/p.jpg" onerror="y()"/><a href="javascript:z()">j</a><a href="two.xhtml#n">n</a>
      <iframe src="https://example.com"/><form action="https://example.com"><input/></form></body></html>`,
      "OEBPS/text/one.xhtml",
    );
    const html = new XMLSerializer().serializeToString(doc);
    expect(html).not.toMatch(/script|onload|onerror|iframe|form|javascript/);
    expect(res.images.map((i) => i.path)).toEqual(["OEBPS/img/p.jpg"]);
    expect(res.styles.map((s) => s.path)).toEqual(["OEBPS/css/a.css"]);
    expect(res.links.map((l) => l.path)).toEqual([null, "OEBPS/text/two.xhtml"]);
  });

  it("drops imports and url() from stylesheets", () => {
    expect(sanitizeCss(`@import "x.css"; body { background: url(a.png) }`)).toBe(` body { background: none }`);
  });
});
