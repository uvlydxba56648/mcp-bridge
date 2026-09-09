const { chromium } = require('playwright-core');
(async () => {
  const exe = process.env.HOME + '/.cache/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-linux64/chrome-headless-shell';
  const browser = await chromium.launch({ executablePath: exe });
  const page = await browser.newPage({ viewport: { width: 960, height: 640 } });
  await page.goto('file:///home/qy/workSpace/JetBrainsmcp/mockups/screen1-connect.html');
  const info = await page.evaluate(() => {
    const f = document.querySelector('.footer');
    const r = f.getBoundingClientRect();
    return { footerRect: {x:r.x,y:r.y,w:r.width,h:r.height}, bodyH: document.body.scrollHeight, btnVisible: !!document.querySelector('.btn.primary') };
  });
  console.log(JSON.stringify(info));
  await browser.close();
})();
