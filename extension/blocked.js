// The page a blocked site opens during a focus session: which site, and the time left.

const params = new URLSearchParams(location.search);
const host = params.get('host') || 'This site';
const until = Number(params.get('until') || 0);
document.getElementById('title').textContent = `${host} is blocked for focus`;

function clock(seconds) {
  const left = Math.max(0, Math.ceil(seconds));
  const hours = Math.floor(left / 3600);
  const minutes = Math.floor((left % 3600) / 60);
  const secs = String(left % 60).padStart(2, '0');
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${secs}` : `${minutes}:${secs}`;
}

function tick() {
  const left = until - Date.now() / 1000;
  document.getElementById('left').textContent = clock(left);
}
tick();
setInterval(tick, 1000);
