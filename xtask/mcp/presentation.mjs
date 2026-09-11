import assert from 'node:assert/strict';

/** Drive the production dock, playback bar and ribbon flyout with real input. */
export async function checkPresentationSurfaces(browser, url) {
  const open = async (disabled = true) => {
    const page = await browser.newPage();
    await page.goto(url);
    await page.evaluate(async disabled => {
      const {mountScriptSurfaces} = await import('/src/scripts/surfaces.browser.test.tsx');
      window.surfaces = mountScriptSurfaces(disabled);
    }, disabled);
    return page;
  };
  const page = await open();
  try {
    await page.getByRole('button', {name: 'Fillet', exact: true}).hover();
    // The catalog deliberately arrives after the opening deadline.
    await page.waitForTimeout(700);
    await page.evaluate(() => window.surfaces.resolveCatalog());
    await page.getByRole('dialog', {name: 'Fillet example'}).waitFor();
    await page.keyboard.press('Escape');
    await page.getByRole('dialog').waitFor({state: 'detached'});
    assert.equal((await page.evaluate(() => window.surfaces.snapshot())).escapedToCad, 0, 'Hover Escape has priority while focus is still outside the preview');
    await page.locator('span[aria-label="Fillet example"]').focus();
    await page.keyboard.press('ArrowDown');
    await page.getByRole('dialog').waitFor();
    await page.getByRole('button', {name: 'Close feature preview'}).focus();
    await page.keyboard.press('Escape');
    await page.getByRole('dialog').waitFor({state: 'detached'});
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Fillet example');
    await page.waitForTimeout(700);
    assert.equal(await page.getByRole('dialog').count(), 0, 'Escape must not reopen from restored focus');
    await page.keyboard.press('ArrowDown');
    await page.getByRole('dialog').waitFor();
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Close feature preview');
    await page.getByRole('button', {name: 'Close feature preview'}).click();
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Fillet example');

    await page.keyboard.press('ArrowDown');
    await page.getByRole('button', {name: 'Open this script →'}).click();
    await page.getByRole('dialog').waitFor({state: 'detached'});
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Close scripts', 'Opening the full script must transfer keyboard focus into its workspace');
    await page.keyboard.press('Escape');
    await page.getByRole('complementary', {name: 'Scripts'}).waitFor({state: 'detached'});

    await page.getByRole('button', {name: 'Scripts', exact: true}).click();
    await page.getByRole('button', {name: 'Close scripts', exact: true}).focus();
    await page.keyboard.press('Escape');
    await page.getByRole('complementary', {name: 'Scripts'}).waitFor({state: 'detached'});
    await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Scripts');
    await page.getByRole('button', {name: 'Scripts', exact: true}).click();
    await page.getByRole('button', {name: 'Close scripts', exact: true}).focus();
    await page.evaluate(() => {
      document.querySelector('[aria-label="Close scripts"]').click();
      document.querySelector('[aria-label="Outside"]').focus();
    });
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Outside', 'Deferred dismissal must preserve a newer focus choice');
    await page.getByRole('button', {name: 'Scripts', exact: true}).click();
    await page.getByRole('button', {name: 'Run in new design'}).click();
    await page.getByRole('region', {name: 'Playback'}).waitFor();
    await page.getByRole('combobox', {name: 'Presentation speed'}).selectOption('8');
    await page.waitForFunction(() => document.querySelector('[aria-label="Script speed"]')?.value === '8');
    await page.getByRole('combobox', {name: 'Presentation speed'}).selectOption('fast');
    await page.waitForFunction(() => document.querySelector('[aria-label="Script run mode"]')?.value === 'fast');
    assert.equal(await page.getByRole('combobox', {name: 'Script speed', exact: true}).count(), 0);
    await page.getByRole('button', {name: 'Pause', exact: true}).click();
    await page.keyboard.press('Escape');
    await page.getByRole('region', {name: 'Playback'}).waitFor({state: 'detached'});
    const hidden = await page.evaluate(() => window.surfaces.snapshot());
    assert(hidden.playback.paused && hidden.scripts.running && !hidden.playback.stopped && hidden.retained);
    assert.equal(hidden.escapedToCad, 0, 'Companion dismissal must not cancel CAD state');
    assert.equal(hidden.scripts.speed, 2, 'Live speed must not overwrite the next-run preference');
    await page.getByRole('button', {name: 'Show playback controls'}).click();
    await page.getByRole('button', {name: 'Resume', exact: true}).click();
    await page.evaluate(() => window.surfaces.finishRun());
    await page.waitForFunction(() => !window.surfaces.snapshot().scripts.running);
    assert.equal(await page.getByRole('combobox', {name: 'Script speed', exact: true}).inputValue(), '2');
  } finally { await page.close(); }

  const departed = await open();
  try {
    await departed.getByRole('button', {name: 'Fillet', exact: true}).hover();
    await departed.waitForTimeout(700);
    await departed.getByRole('button', {name: 'Outside'}).hover();
    await departed.evaluate(() => window.surfaces.resolveCatalog());
    await departed.waitForTimeout(700);
    assert.equal(await departed.getByRole('dialog').count(), 0, 'A late catalog must not resurrect departed hover intent');
  } finally { await departed.close(); }
  const pending = await open(false);
  try {
    await pending.getByRole('button', {name: 'Fillet', exact: true}).focus();
    await pending.keyboard.press('ArrowDown');
    await pending.keyboard.press('Escape');
    await pending.evaluate(() => window.surfaces.resolveCatalog());
    await pending.waitForTimeout(700);
    assert.equal(await pending.getByRole('dialog').count(), 0, 'Escape cancels an immediate keyboard preview while its catalog is pending');
    assert.equal((await pending.evaluate(() => window.surfaces.snapshot())).escapedToCad, 0);
    await pending.getByRole('button', {name: 'Outside'}).focus();
    await pending.keyboard.press('Escape');
    assert.equal((await pending.evaluate(() => window.surfaces.snapshot())).escapedToCad, 1, 'Ordinary CAD Escape still reaches the model after dismissal');
  } finally { await pending.close(); }
  return ['cold-catalog-hover', 'departed-hover', 'keyboard-open', 'disabled-opener-focus',
    'hover-Escape', 'preview-workspace-focus', 'scoped-Escape', 'live-speed-agreement', 'paused-dismissal', 'retained-design', 'launch-preferences'];
}
