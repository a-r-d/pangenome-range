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
          { text: "Distribution", link: "/DISTRIBUTION" },
          { text: "Architecture", link: "/ARCHITECTURE" },
        ],
      },
      {
        text: "Format and research",
        items: [
          { text: "File Format v1", link: "/FILE_FORMAT_V1" },
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
