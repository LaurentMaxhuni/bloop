// Downloads page: refreshes the version list and the latest Windows and Linux buttons from
// GitHub Releases. The HTML already lists every release, so without JavaScript or GitHub the
// page still works; this only adds releases published since the page was last edited.

const API = 'https://api.github.com/repos/LaurentMaxhuni/bloop/releases?per_page=100';
const CACHE_KEY = 'bloop-releases';
const CACHE_MS = 10 * 60 * 1000;
const DOWNLOAD = /^https:\/\/github\.com\/LaurentMaxhuni\/bloop\/releases\/download\/v[0-9][0-9A-Za-z.+-]*\/[0-9A-Za-z._+-]+$/;
const NOTES = /^https:\/\/github\.com\/LaurentMaxhuni\/bloop\/releases\/tag\/v[0-9][0-9A-Za-z.+-]*$/;

/** The platforms bloop ships, with how each one's file is named. */
const PLATFORMS = [
  { key: 'windows', label: 'Windows', file: (v) => `bloop-setup-${v}.exe` },
  { key: 'linux', label: 'Linux', file: (v) => `bloop-${v}-x86_64.AppImage` },
];

const megabytes = (bytes) => `${(bytes / 1048576).toFixed(1)} MB`;
const dateLabel = (iso) =>
  new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' });

/** GitHub's release JSON reduced to what the page shows; anything unexpected is dropped. */
function simplify(json) {
  const releases = [];
  for (const r of Array.isArray(json) ? json : []) {
    if (r.draft || r.prerelease || typeof r.tag_name !== 'string') continue;
    const version = r.tag_name.replace(/^v/, '');
    if (!/^[0-9][0-9A-Za-z.+-]*$/.test(version) || !NOTES.test(r.html_url)) continue;
    const assets = new Map((r.assets ?? []).map((a) => [a.name, a]));
    const files = {};
    for (const p of PLATFORMS) {
      const name = p.file(version);
      const file = assets.get(name);
      const sum = assets.get(`${name}.sha256`);
      if (file && sum && DOWNLOAD.test(file.browser_download_url) && DOWNLOAD.test(sum.browser_download_url)) {
        files[p.key] = { name, size: file.size, url: file.browser_download_url, sum: sum.browser_download_url };
      }
    }
    if (Object.keys(files).length === 0) continue;
    releases.push({ version, date: r.published_at, notes: r.html_url, files });
  }
  return releases;
}

async function load() {
  try {
    const cached = JSON.parse(sessionStorage.getItem(CACHE_KEY) || 'null');
    if (cached && Date.now() - cached.at < CACHE_MS) return cached.releases;
  } catch {}
  if (new URLSearchParams(location.search).has('offline')) throw new Error('offline (debug flag)');
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 6000);
  try {
    const res = await fetch(API, { signal: controller.signal, headers: { Accept: 'application/vnd.github+json' } });
    if (!res.ok) throw new Error(`GitHub API ${res.status}`);
    const releases = simplify(await res.json());
    if (releases.length === 0) throw new Error('no releases');
    try { sessionStorage.setItem(CACHE_KEY, JSON.stringify({ at: Date.now(), releases })); } catch {}
    return releases;
  } finally {
    clearTimeout(timer);
  }
}

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === 'class') node.className = value;
    else if (key === 'text') node.textContent = value;
    else node.setAttribute(key, value);
  }
  for (const child of children) node.append(child);
  return node;
}

function releaseItem(release, latest) {
  const head = el('div', { class: 'release-head' }, [el('h3', { text: release.version })]);
  if (latest) head.append(el('span', { class: 'badge', text: 'Latest' }));
  head.append(
    el('time', { datetime: release.date.slice(0, 10), text: dateLabel(release.date) }),
    el('a', { class: 'notes', href: release.notes, text: 'Release notes' }),
  );
  const files = el('ul', { class: 'files' });
  for (const p of PLATFORMS) {
    const file = release.files[p.key];
    files.append(
      file
        ? el('li', {}, [
            el('span', { class: 'os', text: p.label }),
            el('a', { class: 'name', href: file.url, text: file.name }),
            el('span', { class: 'size', text: megabytes(file.size) }),
            el('a', { class: 'sum', href: file.sum, text: 'SHA-256' }),
          ])
        : el('li', {}, [
            el('span', { class: 'os', text: p.label }),
            el('span', { class: 'none', text: 'Not available for this version' }),
          ]),
    );
  }
  return el('li', { class: 'release' }, [head, files]);
}

function render(releases) {
  const list = document.querySelector('[data-versions]');
  if (list) list.replaceChildren(...releases.map((r, i) => releaseItem(r, i === 0)));
  for (const p of PLATFORMS) {
    // The newest release that has a file for this platform.
    const release = releases.find((r) => r.files[p.key]);
    const button = document.querySelector(`[data-latest="${p.key}"]`);
    const label = document.querySelector(`[data-latest-file="${p.key}"]`);
    if (!release || !button || !label) continue;
    const file = release.files[p.key];
    button.href = file.url;
    button.hidden = false;
    label.textContent = `${file.name} · ${megabytes(file.size)}`;
  }
}

load().then(render).catch(() => {
  // Keep the list and buttons written into the page.
});
