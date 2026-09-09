const { chromium } = require('playwright-core');
(async () => {
  const exe = process.env.HOME + '/.cache/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-linux64/chrome-headless-shell';
  const browser = await chromium.launch({ executablePath: exe });
  const page = await browser.newPage({ viewport: { width: 880, height: 600 }, deviceScaleFactor: 2 });
  const screens = ['screen1-connect','screen1-tools','screen2-tunnel','screen3-ready'];
  for (const s of screens) {
    await page.goto('file:///home/qy/workSpace/JetBrainsmcp/mockups/' + s + '.html');
    await page.waitForTimeout(300);
    await page.screenshot({ path: '/home/qy/workSpace/JetBrainsmcp/screenshots/' + s + '.png' });
    console.log('shot', s);
  }
  await browser.close();
})();
