const { chromium } = require('playwright-core');
(async () => {
  const exe = process.env.HOME + '/.cache/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-linux64/chrome-headless-shell';
  const browser = await chromium.launch({ executablePath: exe });
  const page = await browser.newPage({ viewport: { width: 880, height: 600 }, deviceScaleFactor: 2 });
  const out = '/home/qy/workSpace/JetBrainsmcp/screenshots/';

  await page.goto('http://localhost:4173/');
  await page.waitForTimeout(1400);                    // 等自动检测完成
  await page.screenshot({ path: out + 'app-s1-connect.png' });

  await page.getByText('41 个工具 ▸').click();        // 打开工具弹窗
  await page.waitForTimeout(400);
  await page.screenshot({ path: out + 'app-s1-tools.png' });
  await page.getByRole('button', { name: '关闭' }).click();
  await page.waitForTimeout(200);

  await page.getByRole('button', { name: '继续' }).click();   // → S2(自动安装+建隧道)
  await page.waitForTimeout(3600);                            // 等自动安装 + 隧道建立
  await page.screenshot({ path: out + 'app-s2-tunnel.png' });

  await page.getByRole('button', { name: '继续' }).click();   // → S3
  await page.waitForTimeout(500);
  await page.screenshot({ path: out + 'app-s3-ready.png' });

  await browser.close();
  console.log('all done');
})().catch(e => { console.error(e.message); process.exit(1); });
