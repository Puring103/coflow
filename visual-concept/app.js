(() => {
  const root = document.documentElement;
  const header = document.querySelector('[data-header]');
  const themeToggle = document.querySelector('[data-theme-toggle]');
  const themeQuery = matchMedia('(prefers-color-scheme: dark)');

  /* ---------- 主题切换 ---------- */
  const updateThemeControl = () => {
    const dark = root.dataset.theme === 'dark';
    themeToggle?.setAttribute('aria-label', dark ? '切换到浅色模式' : '切换到深色模式');
    themeToggle?.setAttribute('aria-pressed', String(dark));
  };

  themeToggle?.addEventListener('click', () => {
    const next = root.dataset.theme === 'dark' ? 'light' : 'dark';
    root.dataset.theme = next;
    localStorage.setItem('coflow-theme', next);
    updateThemeControl();
  });

  themeQuery.addEventListener('change', (event) => {
    if (localStorage.getItem('coflow-theme')) return;
    root.dataset.theme = event.matches ? 'dark' : 'light';
    updateThemeControl();
  });

  /* ---------- 滚动时的 header 背景 ---------- */
  const updateHeader = () => header?.classList.toggle('is-scrolled', scrollY > 12);
  window.addEventListener('scroll', updateHeader, { passive: true });

  /* ---------- 卡片叠放：把选中的卡片翻到最前，其余按环形顺序露出边缘 ---------- */
  document.querySelectorAll('[data-cardstack]').forEach((stack) => {
    const tabs = [...stack.querySelectorAll('.stack-tabs button[data-view]')];
    const cards = [...stack.querySelectorAll('.stack-card')];
    const order = cards.map((c) => c.dataset.card);

    const activate = (view) => {
      const front = order.indexOf(view);
      if (front < 0) return;
      tabs.forEach((t) => t.classList.toggle('is-active', t.dataset.view === view));
      cards.forEach((card) => {
        // 相对最前卡片的环形距离 → 叠放深度
        const depth = (order.indexOf(card.dataset.card) - front + order.length) % order.length;
        card.dataset.depth = String(depth);
      });
    };

    // 点击后面的卡片也能置顶
    cards.forEach((card) => {
      card.addEventListener('click', () => {
        if (card.dataset.depth !== '0') activate(card.dataset.card);
      });
    });
    tabs.forEach((tab) => {
      tab.addEventListener('click', () => activate(tab.dataset.view));
      tab.addEventListener('mouseenter', () => activate(tab.dataset.view));
      tab.addEventListener('focus', () => activate(tab.dataset.view));
    });

    // 初始：以标记为 is-active 的标签（或第一张）为最前
    const initial = tabs.find((t) => t.classList.contains('is-active')) || tabs[0];
    if (initial) activate(initial.dataset.view);
  });

  /* ---------- 进入视口时的揭示动效 ---------- */
  const reveals = document.querySelectorAll('[data-reveal]');
  const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const forceShow = location.search.includes('reveal=off');

  if (reduceMotion || forceShow || !('IntersectionObserver' in window)) {
    reveals.forEach((el) => el.classList.add('is-visible'));
  } else {
    const observer = new IntersectionObserver(
      (entries, obs) => {
        entries.forEach((entry, i) => {
          if (!entry.isIntersecting) return;
          const delay = entry.target.dataset.revealDelay || i * 60;
          setTimeout(() => entry.target.classList.add('is-visible'), delay);
          obs.unobserve(entry.target);
        });
      },
      { rootMargin: '0px 0px -12% 0px', threshold: 0.12 }
    );
    reveals.forEach((el) => observer.observe(el));
  }

  updateThemeControl();
  updateHeader();
})();
