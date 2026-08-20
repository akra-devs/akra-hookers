import { expect, test, type Locator, type Page } from "@playwright/test";

import { installFixtureApi } from "../tests/fixtures/api";
import { createShowcaseState } from "./showcase-state";

const pause = (milliseconds: number) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const RECORDING_ZOOM = 1.6;

async function installShowcaseChrome(page: Page) {
  await page.addStyleTag({ content: `
    #showcase-slate,
    #showcase-chapter,
    #showcase-cursor {
      pointer-events: none;
      position: fixed;
      z-index: 2147483646;
    }
    #showcase-slate {
      inset: 0;
      display: grid;
      place-items: center;
      background:
        radial-gradient(circle at 73% 32%, rgba(71, 130, 177, .18), transparent 32%),
        radial-gradient(circle at 25% 66%, rgba(126, 213, 169, .12), transparent 36%),
        rgba(4, 9, 11, .94);
      backdrop-filter: blur(16px);
      opacity: 0;
      transition: opacity 420ms ease;
    }
    #showcase-slate.is-visible { opacity: 1; }
    #showcase-slate > div {
      width: min(860px, 78vw);
      border-left: 2px solid #7ed5a9;
      padding: 18px 0 18px 38px;
    }
    #showcase-slate small,
    #showcase-chapter small {
      display: block;
      color: #7ed5a9;
      font: 700 12px/1.2 ui-monospace, SFMono-Regular, Consolas, monospace;
      letter-spacing: .16em;
      text-transform: uppercase;
    }
    #showcase-slate h2 {
      max-width: 780px;
      margin: 20px 0 18px;
      color: #f1f5f2;
      font: 700 clamp(48px, 5.5vw, 78px)/1.02 Georgia, serif;
      letter-spacing: -.04em;
      white-space: pre-line;
    }
    #showcase-slate p {
      margin: 0;
      color: #a9b8b8;
      font: 500 17px/1.5 system-ui, sans-serif;
    }
    #showcase-chapter {
      top: 88px;
      left: 286px;
      width: 470px;
      padding: 18px 20px 19px;
      border: 1px solid #314247;
      border-left: 3px solid #7ed5a9;
      background: rgba(7, 15, 18, .94);
      box-shadow: 0 20px 60px rgba(0, 0, 0, .42);
      opacity: 0;
      transform: translateY(-10px);
      transition: opacity 260ms ease, transform 260ms ease;
    }
    #showcase-chapter.is-visible {
      opacity: 1;
      transform: translateY(0);
    }
    #showcase-chapter strong {
      display: block;
      margin-top: 8px;
      color: #f1f5f2;
      font: 700 24px/1.18 Georgia, serif;
    }
    #showcase-chapter p {
      margin: 8px 0 0;
      color: #9fb0b1;
      font: 500 13px/1.45 system-ui, sans-serif;
    }
    #showcase-cursor {
      top: 0;
      left: 0;
      width: 22px;
      height: 22px;
      border: 2px solid #f4faf6;
      border-radius: 50%;
      background: rgba(126, 213, 169, .2);
      box-shadow: 0 0 0 4px rgba(126, 213, 169, .1), 0 8px 24px rgba(0, 0, 0, .45);
      opacity: 0;
      transform: translate3d(-40px, -40px, 0);
      transition: transform 520ms cubic-bezier(.2, .8, .2, 1), opacity 160ms ease;
    }
    #showcase-cursor.is-visible { opacity: 1; }
    #showcase-cursor::after {
      content: "";
      position: absolute;
      inset: -8px;
      border: 1px solid #7ed5a9;
      border-radius: inherit;
      opacity: 0;
    }
    #showcase-cursor.is-clicking::after { animation: showcase-click 360ms ease-out; }
    @keyframes showcase-click {
      0% { opacity: .9; transform: scale(.7); }
      100% { opacity: 0; transform: scale(1.55); }
    }
    [data-showcase-focus="true"] {
      outline: 2px solid #7ed5a9 !important;
      outline-offset: 4px !important;
      box-shadow: 0 0 0 8px rgba(126, 213, 169, .09) !important;
    }
    @media (prefers-reduced-motion: reduce) {
      #showcase-slate, #showcase-chapter, #showcase-cursor { transition-duration: 1ms; }
      #showcase-cursor.is-clicking::after { animation: none; }
    }
  ` });
  await page.evaluate(() => {
    const slate = document.createElement("section");
    slate.id = "showcase-slate";
    slate.innerHTML = "<div><small></small><h2></h2><p></p></div>";
    const chapter = document.createElement("section");
    chapter.id = "showcase-chapter";
    chapter.innerHTML = "<small></small><strong></strong><p></p>";
    const cursor = document.createElement("div");
    cursor.id = "showcase-cursor";
    cursor.setAttribute("aria-hidden", "true");
    document.body.append(slate, chapter, cursor);
  });
}

async function showSlate(
  page: Page,
  eyebrow: string,
  title: string,
  description: string,
  duration = 3_400,
) {
  await page.evaluate(({ eyebrow, title, description }) => {
    const slate = document.querySelector<HTMLElement>("#showcase-slate")!;
    slate.querySelector("small")!.textContent = eyebrow;
    slate.querySelector("h2")!.textContent = title;
    slate.querySelector("p")!.textContent = description;
    slate.classList.add("is-visible");
  }, { eyebrow, title, description });
  await pause(duration);
  await page.evaluate(() => document.querySelector("#showcase-slate")?.classList.remove("is-visible"));
  await pause(480);
}

async function showChapter(
  page: Page,
  number: string,
  title: string,
  description: string,
) {
  await page.evaluate(({ number, title, description }) => {
    const chapter = document.querySelector<HTMLElement>("#showcase-chapter")!;
    chapter.querySelector("small")!.textContent = number;
    chapter.querySelector("strong")!.textContent = title;
    chapter.querySelector("p")!.textContent = description;
    chapter.classList.add("is-visible");
  }, { number, title, description });
  await pause(2_100);
  await page.evaluate(() => document.querySelector("#showcase-chapter")?.classList.remove("is-visible"));
  await pause(360);
}

async function pointAt(page: Page, target: Locator, settle = 560) {
  await target.scrollIntoViewIfNeeded();
  const box = await target.boundingBox();
  if (!box) throw new Error("Showcase target is not measurable");
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.evaluate(({ x, y, zoom }) => {
    document.querySelectorAll("[data-showcase-focus]").forEach((element) => {
      element.removeAttribute("data-showcase-focus");
    });
    const cursor = document.querySelector<HTMLElement>("#showcase-cursor")!;
    cursor.classList.add("is-visible");
    cursor.style.transform = `translate3d(${x / zoom - 11}px, ${y / zoom - 11}px, 0)`;
  }, { x, y, zoom: RECORDING_ZOOM });
  await target.evaluate((element) => element.setAttribute("data-showcase-focus", "true"));
  await pause(settle);
}

async function clickTarget(page: Page, target: Locator, after = 620) {
  await pointAt(page, target);
  await target.click();
  await page.evaluate(() => {
    const cursor = document.querySelector<HTMLElement>("#showcase-cursor")!;
    cursor.classList.remove("is-clicking");
    void cursor.offsetWidth;
    cursor.classList.add("is-clicking");
  });
  await pause(after);
}

async function selectTarget(page: Page, target: Locator, value: string, after = 760) {
  await pointAt(page, target);
  await target.selectOption(value);
  await pause(after);
}

async function clearFocus(page: Page) {
  await page.evaluate(() => {
    document.querySelectorAll("[data-showcase-focus]").forEach((element) => {
      element.removeAttribute("data-showcase-focus");
    });
  });
}

async function connectWorkNodes(page: Page, sourceWorkId: number, targetWorkId: number) {
  const source = page.getByTestId(`work-node-${sourceWorkId}`).locator(".react-flow__handle.source");
  const target = page.getByTestId(`work-node-${targetWorkId}`).locator(".react-flow__handle.target");
  const sourceBox = await source.boundingBox();
  const targetBox = await target.boundingBox();
  if (!sourceBox || !targetBox) throw new Error("Work handles must be measurable");
  const sourcePoint = {
    x: sourceBox.x + sourceBox.width / 2,
    y: sourceBox.y + sourceBox.height / 2,
  };
  const targetPoint = {
    x: targetBox.x + targetBox.width / 2,
    y: targetBox.y + targetBox.height / 2,
  };
  await page.evaluate(({ x, y, zoom }) => {
    const cursor = document.querySelector<HTMLElement>("#showcase-cursor")!;
    cursor.classList.add("is-visible");
    cursor.style.transform = `translate3d(${x / zoom - 11}px, ${y / zoom - 11}px, 0)`;
  }, { ...sourcePoint, zoom: RECORDING_ZOOM });
  await pause(520);
  await page.mouse.move(sourcePoint.x, sourcePoint.y);
  await page.mouse.down();
  await page.evaluate(({ x, y, zoom }) => {
    const cursor = document.querySelector<HTMLElement>("#showcase-cursor")!;
    cursor.style.transform = `translate3d(${x / zoom - 11}px, ${y / zoom - 11}px, 0)`;
  }, { ...targetPoint, zoom: RECORDING_ZOOM });
  await page.mouse.move(targetPoint.x, targetPoint.y, { steps: 22 });
  await pause(250);
  await page.mouse.up();
  await pause(780);
}

test("record the Akra Hookers product showcase", async ({ page }) => {
  await page.addInitScript(() => {
    const applyRecordingZoom = () => {
      if (!document.documentElement) return false;
      document.documentElement.style.zoom = "1.6";
      return true;
    };
    if (!applyRecordingZoom()) {
      const observer = new MutationObserver(() => {
        if (applyRecordingZoom()) observer.disconnect();
      });
      observer.observe(document, { childList: true, subtree: true });
    }
    window.localStorage.setItem(
      "akra.canvas.activity-visibility.v1",
      JSON.stringify({ subagent: true, internal: false }),
    );
  });
  await installFixtureApi(page, createShowcaseState());
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Prompt canvas" })).toBeVisible();
  await expect(page.getByTestId("activity-node-10")).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await installShowcaseChrome(page);

  await showSlate(
    page,
    "AKRA HOOKERS · PRODUCT SHOWCASE",
    "흩어진 대화를\n검증 가능한 작업으로.",
    "실제 제품 UI · 격리된 데모 데이터 · 외부 LLM 호출 없음",
    4_000,
  );

  await showChapter(
    page,
    "01 · CAPTURE",
    "수집 상태를 한눈에",
    "Codex App, CLI, WSL의 hook 상태와 최근 수집을 한 화면에서 확인합니다.",
  );
  const health = page.getByRole("button", { name: /Capture health:/ });
  await clickTarget(page, health, 900);
  const captureSection = page.locator("#capture-settings");
  await pointAt(page, captureSection, 1_200);
  await clearFocus(page);
  await page.locator(".rail").evaluate((element) => element.scrollTo({ top: 0, behavior: "smooth" }));
  await pause(700);

  await showChapter(
    page,
    "02 · FILTER",
    "시간과 맥락을 함께 좁히기",
    "오늘과 최근 24시간을 구분하고, 숫자와 프로젝트 목록도 같은 조건으로 갱신합니다.",
  );
  const period = page.getByLabel("기간 필터");
  await selectTarget(page, period, "today", 1_000);
  await expect(page.locator(".rail-projects").getByText("Waxball", { exact: true })).toBeVisible();
  const hideEmpty = page.getByRole("checkbox", { name: "결과 없는 프로젝트 숨기기" });
  await clickTarget(page, hideEmpty, 850);
  await expect(page.locator(".rail-projects").getByText("Waxball", { exact: true })).toHaveCount(0);
  await selectTarget(page, period, "day", 1_000);
  await expect(page.locator(".rail-projects").getByText("Waxball", { exact: true })).toBeVisible();
  const subagent = page.getByRole("checkbox", { name: /Subagent activity/ });
  await clickTarget(page, subagent, 850);
  await expect(subagent).not.toBeChecked();

  await showChapter(
    page,
    "03 · EVIDENCE",
    "요약과 원문을 함께 검증",
    "짧은 요청 요약으로 탐색하고, 상세에서 원문·결과·대화 흐름을 필요할 때만 펼칩니다.",
  );
  await selectTarget(page, page.getByLabel("프로젝트 필터"), "project:1", 850);
  await selectTarget(page, period, "today", 850);
  await clickTarget(page, page.getByTestId("activity-node-7"), 900);
  const detail = page.getByTestId("activity-detail-panel");
  await expect(detail).toBeVisible();
  const regenerate = detail.getByRole("button", { name: "결과 요약 재생성" });
  await clickTarget(page, regenerate, 260);
  await expect(detail).toContainText("로딩...");
  await expect(detail).toContainText("보관 중인 응답으로 결과 요약을 다시 만들었습니다.");
  await pointAt(page, detail.getByTestId("activity-result-summary"), 950);
  await clickTarget(page, detail.getByRole("button", { name: "대화 기록 크게 보기" }), 700);
  const conversationDialog = page.getByRole("dialog", { name: "대화 흐름" });
  await expect(conversationDialog).toBeVisible();
  await pointAt(page, conversationDialog.locator(".activity-conversation-dialog__timeline"), 1_300);
  await clickTarget(page, conversationDialog.getByRole("button", { name: "대화 흐름 닫기" }), 500);
  await clickTarget(page, detail.getByRole("button", { name: "상세 닫기" }), 550);

  await showChapter(
    page,
    "04 · CURATE",
    "필요한 로그만 작업 후보로",
    "전체 원문 대신 최대 96자 요청 요약과 3줄 결과만 Spark에 전달합니다.",
  );
  await clickTarget(page, page.getByRole("button", { name: "로그 정리" }), 750);
  const workspace = page.getByTestId("curation-workspace");
  await expect(workspace).toBeVisible();
  const selectAll = workspace.getByRole("checkbox", { name: /전체 선택/ });
  await clickTarget(page, selectAll, 550);
  await expect(workspace.getByText("3개 선택", { exact: true })).toBeVisible();
  const firstDetails = workspace.locator(".curation-log__details").first();
  await clickTarget(page, firstDetails.getByText("더보기", { exact: true }), 750);
  await pointAt(page, firstDetails.locator(".curation-log__evidence"), 1_000);
  await pointAt(page, workspace.getByRole("button", { name: "로그 영구 제외" }).first(), 700);
  await clickTarget(page, workspace.getByRole("button", { name: "선택한 3개 자동 묶기" }), 800);

  await showChapter(
    page,
    "05 · REVIEW",
    "AI는 제안하고, 사람은 확정",
    "관련 로그는 기존 작업에 합치고, 다른 목적의 로그는 새 작업으로 분리합니다.",
  );
  await expect(workspace.getByRole("heading", { name: "AI가 제안한 작업 묶음" })).toBeVisible();
  await clickTarget(page, workspace.getByRole("button", { name: "새 작업으로 분리" }), 550);
  const releaseLog = workspace.locator(".curation-group li").filter({
    hasText: "실제 화면 중심 배포 페이지 구성",
  });
  await selectTarget(page, releaseLog.getByLabel("로그를 옮길 작업"), "1", 650);
  const newTitle = workspace.locator(".curation-group input").nth(1);
  await pointAt(page, newTitle, 350);
  await newTitle.fill("배포 경험 고도화");
  await pause(800);
  await clickTarget(page, workspace.getByRole("button", { name: "검토한 제안 적용" }), 800);
  await expect(workspace).toContainText("2개 작업을 Project Memory에 반영했습니다.");
  await clickTarget(page, workspace.getByRole("button", { name: "작업 지도에서 보기" }), 1_000);
  await expect(page.getByTestId("work-node-3")).toBeVisible();

  await showChapter(
    page,
    "06 · RELATE",
    "관계는 직접 만들고 지웁니다",
    "AI가 임의로 선을 만들지 않습니다. 사용자가 확인한 작업 사이에만 관계를 남깁니다.",
  );
  const created = page.waitForResponse((response) =>
    response.request().method() === "POST"
    && new URL(response.url()).pathname === "/v1/work-items/edges",
  );
  await connectWorkNodes(page, 1, 3);
  expect((await created).status()).toBe(201);
  const edge = page.locator(".project-memory-stage .react-flow__edge").first();
  await expect(edge).toBeVisible();
  await pointAt(page, edge, 900);
  await edge.locator(".react-flow__edge-interaction").click({ force: true });
  await page.keyboard.press("Delete");
  await expect(page.locator(".project-memory-stage .react-flow__edge")).toHaveCount(0);
  await pause(650);
  await connectWorkNodes(page, 1, 3);
  await expect(page.locator(".project-memory-stage .react-flow__edge")).toHaveCount(1);
  await clickTarget(page, page.getByTestId("work-node-3"), 900);
  await expect(page.getByTestId("work-detail-panel")).toContainText("배포 경험 고도화");
  await pointAt(page, page.getByTestId("work-detail-panel").getByRole("list", { name: "결과 요약" }), 1_200);
  await clearFocus(page);

  await showSlate(
    page,
    "AKRA HOOKERS",
    "필요한 로그만,\n맥락이 남는 작업으로.",
    "akra.kr/akra-hookers",
    4_200,
  );
});
