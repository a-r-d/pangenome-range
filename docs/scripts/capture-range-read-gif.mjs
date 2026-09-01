import { spawn } from "node:child_process";
import { mkdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const docsDirectory = dirname(dirname(fileURLToPath(import.meta.url)));
const outputPath = join(docsDirectory, "public", "format-range-read.gif");
const urlArgument = process.argv
  .find((argument) => argument.startsWith("--url="))
  ?.slice("--url=".length);
const baseUrl =
  urlArgument ??
  process.env.RANGE_READ_GIF_URL ??
  "http://127.0.0.1:5173/pangenome-range";
const pageUrl = `${baseUrl.replace(/\/$/, "")}/how-range-reads-work?capture=1`;
const frameIntervalMs = 100;

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({
  viewport: { width: 1100, height: 800 },
  deviceScaleFactor: 1,
});

try {
  await page.goto(pageUrl, { waitUntil: "domcontentloaded", timeout: 60_000 });
  await page.waitForFunction(
    () => window.__pngrRangeReadAnimation?.ready === true,
    undefined,
    { timeout: 30_000 },
  );
  const duration = await page.evaluate(
    () => window.__pngrRangeReadAnimation?.duration ?? 13_000,
  );
  await page.evaluate(() => {
    window.__pngrRangeReadAnimation?.setTime(0);
    for (const selector of [
      ".VPNav",
      ".VPSidebar",
      ".VPLocalNav",
      ".VPBackdrop",
      ".edit-link",
      "footer",
    ]) {
      for (const node of document.querySelectorAll(selector)) {
        if (node instanceof HTMLElement) node.style.display = "none";
      }
    }
    const figure = document.querySelector(".range-read-animation");
    if (figure instanceof HTMLElement) {
      document.body.prepend(figure);
      document.documentElement.classList.add("dark");
      document.body.style.background = "#0b1220";
      document.body.style.margin = "0";
      document.body.style.overflow = "hidden";
    }
  });
  await new Promise((resolve) => setTimeout(resolve, 120));

  const frameDirectory = join(tmpdir(), `pngr-range-gif-${Date.now()}`);
  await mkdir(frameDirectory, { recursive: true });
  const locator = page.locator(".range-read-animation__capture");
  await locator.waitFor({ state: "visible" });

  let frame = 0;
  for (let time = 0; time <= duration; time += frameIntervalMs) {
    await page.evaluate((ms) => {
      window.__pngrRangeReadAnimation?.setTime(ms);
    }, time);
    await page.evaluate(
      () =>
        new Promise((resolve) => {
          requestAnimationFrame(() => requestAnimationFrame(resolve));
        }),
    );
    await locator.screenshot({
      path: join(frameDirectory, `frame-${String(frame).padStart(4, "0")}.png`),
      animations: "disabled",
    });
    frame += 1;
  }

  await encodeGif(frameDirectory, outputPath);
  await rm(frameDirectory, { recursive: true, force: true });
  console.log(`wrote ${outputPath} from ${frame} frames at ${pageUrl}`);
} finally {
  await browser.close();
}

function encodeGif(frameDirectory, gifPath) {
  return new Promise((resolve, reject) => {
    const child = spawn(
      "ffmpeg",
      [
        "-y",
        "-framerate",
        "10",
        "-i",
        join(frameDirectory, "frame-%04d.png"),
        "-vf",
        "fps=10,scale=960:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=96:stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=4",
        "-loop",
        "0",
        gifPath,
      ],
      { stdio: "inherit" },
    );
    child.once("error", reject);
    child.once("exit", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`ffmpeg exited ${code}`));
    });
  });
}
