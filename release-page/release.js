const repository = "akra-devs/akra-hookers";
const version = document.querySelector("#release-version");
const date = document.querySelector("#release-date");
const status = document.querySelector("#release-status");
const downloads = [...document.querySelectorAll(".release-download")];
const checksum = document.querySelector("#checksum");

async function loadRelease() {
  try {
    const response = await fetch(`https://api.github.com/repos/${repository}/releases/latest`, { headers: { Accept: "application/vnd.github+json" } });
    if (!response.ok) throw new Error(`GitHub API ${response.status}`);
    const release = await response.json();
    const archive = release.assets.find(({ name }) => name === "Akra-Hookers-Windows-x64-portable.zip");
    const digest = release.assets.find(({ name }) => name.endsWith(".sha256"));
    if (!archive) throw new Error("portable ZIP이 아직 게시되지 않았습니다.");
    version.textContent = `${release.tag_name} · WINDOWS X64`;
    date.textContent = `${new Intl.DateTimeFormat("ko-KR", { dateStyle: "long" }).format(new Date(release.published_at))} 게시`;
    for (const link of downloads) { link.href = archive.browser_download_url; link.classList.remove("disabled"); link.removeAttribute("aria-disabled"); }
    if (digest) checksum.href = digest.browser_download_url;
    status.textContent = `${(archive.size / 1024 / 1024).toFixed(1)} MB · GitHub Releases에서 직접 다운로드`;
  } catch (error) {
    version.textContent = "릴리스 정보를 확인하지 못했습니다";
    date.textContent = "다운로드 버튼에서 GitHub 최신 릴리스를 확인할 수 있습니다.";
    status.textContent = "릴리스 메타데이터를 불러오지 못했지만 다운로드 경로는 사용할 수 있습니다.";
  }
}

void loadRelease();
