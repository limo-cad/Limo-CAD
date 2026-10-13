const films = [...document.querySelectorAll('.showcases video')];
const sections = [...document.querySelectorAll('.showcases > section')];
function selectedExample() {
  let id;
  try { id = decodeURIComponent(location.hash.slice(1)); } catch { return null; }
  return sections.find(section => section.id === id) ?? null;
}
function pauseHiddenFilms() {
  const selected = selectedExample();
  if (selected) for (const film of films) if (!selected.contains(film)) film.pause();
}
for (const film of films) film.addEventListener('play', () => {
  for (const other of films) if (other !== film) other.pause();
  pauseHiddenFilms();
});
addEventListener('hashchange', pauseHiddenFilms);
pauseHiddenFilms();
