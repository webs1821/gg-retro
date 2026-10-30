// GG-Retro UI

function showSection(event, sectionId) {
  document.querySelectorAll('.section').forEach(s => s.classList.remove('active'));
  document.querySelectorAll('.sidebar-tab').forEach(t => t.classList.remove('active'));
  document.getElementById(sectionId).classList.add('active');
  event.currentTarget.classList.add('active');
}

// Window dragging (desktop only)
document.addEventListener('DOMContentLoaded', function() {
  if (window.innerWidth <= 768) return;

  const titlebar = document.querySelector('.titlebar');
  const windowEl = document.querySelector('.window');
  let isDragging = false, initialX, initialY;

  titlebar.addEventListener('mousedown', (e) => {
    if (e.target.classList.contains('titlebar-button')) return;
    isDragging = true;
    initialX = e.clientX - windowEl.offsetLeft;
    initialY = e.clientY - windowEl.offsetTop;
  });

  document.addEventListener('mousemove', (e) => {
    if (!isDragging) return;
    e.preventDefault();
    windowEl.style.left = (e.clientX - initialX) + 'px';
    windowEl.style.top = (e.clientY - initialY) + 'px';
    windowEl.style.transform = 'none';
  });

  document.addEventListener('mouseup', () => isDragging = false);
});
