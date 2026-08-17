const repository = "akra-devs/akra-hookers";
const version = document.querySelector("#release-version");
const date = document.querySelector("#release-date");
const status = document.querySelector("#release-status");
const download = document.querySelector("#download");

async function loadRelease() {
  try {
    const response = await fetch(`https://api.github.com/repos/${repository}/releases/latest`, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) throw new Error(`GitHub API ${response.status}`);
    const release = await response.json();
    const asset = release.assets.find(({ name }) => name === "Akra-Hookers-Windows-x64-portable.zip");
    if (!asset) throw new Error("portable ZIP이 아직 게시되지 않았습니다.");
    version.textContent = release.tag_name;
    date.textContent = new Intl.DateTimeFormat("ko-KR", { dateStyle: "long" }).format(new Date(release.published_at));
    download.href = asset.browser_download_url;
    download.classList.remove("disabled");
    download.removeAttribute("aria-disabled");
    status.textContent = `${(asset.size / 1024 / 1024).toFixed(1)} MB · GitHub Releases에서 직접 다운로드`;
  } catch (error) {
    version.textContent = "릴리스 준비 중";
    date.textContent = "첫 desktop-v* 태그가 게시되면 다운로드가 활성화됩니다.";
    status.textContent = error instanceof Error ? error.message : "릴리스 정보를 확인하지 못했습니다.";
  }
}

void loadRelease();
