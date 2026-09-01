import { defineConfig } from "vitepress";

export default defineConfig({
  title: "pangenome-range",
  description:
    "A range-addressable static-object representation for regional pangenome graph queries",
  base: "/pangenome-range/",
  cleanUrls: true,
  themeConfig: {
    nav: [
      { text: "Guide", link: "/" },
      { text: "Demo", link: "/demo" },
      { text: "How range reads work", link: "/how-range-reads-work" },
      { text: "File Format v1", link: "/FILE_FORMAT_V1" },
      { text: "Architecture", link: "/ARCHITECTURE" },
      { text: "Benchmarks", link: "/BENCHMARKS" },
    ],
    sidebar: [
      {
        text: "Project",
        items: [
          { text: "Overview", link: "/" },
          { text: "Demo", link: "/demo" },
          { text: "How range reads work", link: "/how-range-reads-work" },
          { text: "Hosting", link: "/HOSTING" },
          { text: "Distribution", link: "/DISTRIBUTION" },
          { text: "Architecture", link: "/ARCHITECTURE" },
          { text: "Viewer requirements", link: "/VIEWER_PRODUCT_REQUIREMENTS" },
          { text: "Viewer performance", link: "/VIEWER_PERFORMANCE" },
          { text: "Viewer format gaps", link: "/VIEWER_FORMAT_GAPS" },
        ],
      },
      {
        text: "Format and research",
        items: [
          { text: "File Format v1", link: "/FILE_FORMAT_V1" },
          { text: "How range reads work", link: "/how-range-reads-work" },
          { text: "Fixed-window archive", link: "/FIXED_WINDOW_ARCHIVE" },
          { text: "Format goals", link: "/FORMAT_GOALS" },
          { text: "Research", link: "/RESEARCH" },
          { text: "Upstream notes", link: "/UPSTREAM" },
        ],
      },
      {
        text: "Evidence",
        items: [
          { text: "Benchmarks", link: "/BENCHMARKS" },
          { text: "Optimization log", link: "/OPTIMIZATION_LOG" },
        ],
      },
    ],
    socialLinks: [
      {
        icon: "github",
        link: "https://github.com/a-r-d/pangenome-range",
      },
    ],
    editLink: {
      pattern: "https://github.com/a-r-d/pangenome-range/edit/main/docs/:path",
    },
    search: { provider: "local" },
  },
});
