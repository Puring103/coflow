(() => {
  const header = document.querySelector('[data-header]');
  const canvas = document.querySelector('[data-flow-field]');
  const context = canvas?.getContext('2d');
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const pointer = { x: 0.68, y: 0.43, active: false };
  let frame = 0;
  let width = 0;
  let height = 0;
  let dpr = 1;

  const resize = () => {
    if (!canvas || !context) return;
    const bounds = canvas.getBoundingClientRect();
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    width = bounds.width;
    height = bounds.height;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
  };

  const bezierPoint = (t, p0, p1, p2, p3) => {
    const mt = 1 - t;
    return {
      x: mt ** 3 * p0.x + 3 * mt ** 2 * t * p1.x + 3 * mt * t ** 2 * p2.x + t ** 3 * p3.x,
      y: mt ** 3 * p0.y + 3 * mt ** 2 * t * p1.y + 3 * mt * t ** 2 * p2.y + t ** 3 * p3.y
    };
  };

  const drawFlow = (time = 0) => {
    if (!context) return;
    context.clearRect(0, 0, width, height);
    const compact = width < 760;
    const convergeX = width * (compact ? .73 : .7);
    const convergeY = height * (compact ? .31 : .42);
    const influenceX = (pointer.x - .5) * (compact ? 24 : 52);
    const influenceY = (pointer.y - .5) * (compact ? 20 : 42);
    const lineCount = compact ? 15 : 23;
    const startX = width * (compact ? .42 : .48);

    for (let index = 0; index < lineCount; index += 1) {
      const ratio = index / (lineCount - 1);
      const startY = height * (.1 + ratio * .78);
      const spread = (ratio - .5) * height * .12;
      const endY = convergeY + spread * .15;
      const p0 = { x: startX, y: startY };
      const p1 = { x: width * .58 + influenceX * .25, y: startY + influenceY * .2 };
      const p2 = { x: convergeX - width * .06 + influenceX, y: endY + influenceY };
      const p3 = { x: width * 1.06, y: endY + influenceY * .55 };

      context.beginPath();
      context.moveTo(p0.x, p0.y);
      context.bezierCurveTo(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
      const role = ratio < .34 ? '240, 107, 93' : ratio < .67 ? '42, 175, 191' : '185, 230, 58';
      context.strokeStyle = `rgba(${role}, ${0.035 + ratio * 0.014})`;
      context.lineWidth = 1;
      context.stroke();

      const speed = .00007 + ratio * .000025;
      const phase = reduceMotion ? .62 : (time * speed + ratio * .67) % 1;
      const dot = bezierPoint(phase, p0, p1, p2, p3);
      const radius = index % 5 === 0 ? 3.6 : 1.8;
      context.beginPath();
      context.arc(dot.x, dot.y, radius, 0, Math.PI * 2);
      context.fillStyle = ratio < .34 ? '#f06b5d' : ratio < .67 ? '#2aafbf' : '#b9e63a';
      context.fill();
    }

    context.beginPath();
    context.arc(convergeX + influenceX, convergeY + influenceY, compact ? 6 : 8, 0, Math.PI * 2);
    context.fillStyle = '#147c75';
    context.fill();
    context.strokeStyle = 'rgba(24,36,33,.75)';
    context.lineWidth = 1.5;
    context.stroke();

    if (!reduceMotion) frame = requestAnimationFrame(drawFlow);
  };

  window.addEventListener('resize', () => { resize(); if (reduceMotion) drawFlow(); }, { passive: true });
  window.addEventListener('scroll', () => header?.classList.toggle('is-scrolled', window.scrollY > 20), { passive: true });
  canvas?.addEventListener('pointermove', (event) => {
    const bounds = canvas.getBoundingClientRect();
    pointer.x = (event.clientX - bounds.left) / bounds.width;
    pointer.y = (event.clientY - bounds.top) / bounds.height;
    pointer.active = true;
  });
  canvas?.addEventListener('pointerleave', () => { pointer.active = false; pointer.x = .68; pointer.y = .43; });

  const toast = document.querySelector('[data-toast]');
  let toastTimer;
  document.querySelector('[data-copy]')?.addEventListener('click', async (event) => {
    const button = event.currentTarget;
    try {
      await navigator.clipboard.writeText(button.dataset.copy);
      toast?.classList.add('is-visible');
      clearTimeout(toastTimer);
      toastTimer = setTimeout(() => toast?.classList.remove('is-visible'), 1800);
    } catch {
      const command = button.parentElement?.querySelector('code');
      const selection = window.getSelection();
      const range = document.createRange();
      if (command && selection) {
        range.selectNodeContents(command);
        selection.removeAllRanges();
        selection.addRange(range);
      }
    }
  });

  resize();
  drawFlow(performance.now());
})();
