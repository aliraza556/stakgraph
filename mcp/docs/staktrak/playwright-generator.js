/**
 * Generates a Playwright test from tracking data
 * @param {Object} trackingData - The tracking data object
 * @returns {string} - Generated Playwright test code
 */
export function generatePlaywrightTest(url, trackingData) {
  const {
    clicks,
    keyboardActivities,
    inputChanges,
    focusChanges,
    userInfo,
    time,
  } = trackingData;

  if (
    (!clicks || !clicks.clickDetails || clicks.clickDetails.length === 0) &&
    (!inputChanges || inputChanges.length === 0)
  ) {
    return generateEmptyTest(url);
  }

  const testCode = `import { test, expect } from '@playwright/test';
  
  test('User interaction replay', async ({ page }) => {
    // Navigate to the page
    await page.goto('${url}');
    
    // Wait for page to load
    await page.waitForLoadState('networkidle');
    
    // Set viewport size to match recorded session
    await page.setViewportSize({ 
      width: ${userInfo.windowSize[0]}, 
      height: ${userInfo.windowSize[1]} 
    });
  
  ${generateUserInteractions(clicks, inputChanges, focusChanges)}

    await page.waitForTimeout(2500);
  });`;

  return testCode;
}

/**
 * Generates code for all user interactions in chronological order
 * @param {Object} clicks - Click data
 * @param {Array} inputChanges - Input change data
 * @param {Array} focusChanges - Focus change data
 * @returns {string} - Generated interactions code
 */
function generateUserInteractions(clicks, inputChanges, focusChanges) {
  const allEvents = [];

  if (clicks && clicks.clickDetails && clicks.clickDetails.length > 0) {
    clicks.clickDetails.forEach((clickDetail) => {
      const [x, y, selector, timestamp] = clickDetail;
      allEvents.push({
        type: "click",
        x,
        y,
        selector,
        timestamp,
      });
    });
  }

  const inputEvents = [];
  if (inputChanges && inputChanges.length > 0) {
    const completedInputs = inputChanges.filter(
      (change) => change.action === "complete" || !change.action
    );

    completedInputs.forEach((change) => {
      inputEvents.push({
        type: "input",
        selector: change.elementSelector,
        value: change.value,
        timestamp: change.timestamp,
      });
    });

    allEvents.push(...inputEvents);
  }

  allEvents.sort((a, b) => a.timestamp - b.timestamp);

  let actionsCode = "";
  let previousTimestamp = null;
  let generatedSelectors = new Set();

  allEvents.forEach((event, index) => {
    if (previousTimestamp !== null) {
      const delay = event.timestamp - previousTimestamp;
      if (delay > 100) {
        // Only add delay if it's significant
        actionsCode += `  
    // Wait ${delay}ms (matching user timing)
    await page.waitForTimeout(${delay});
  `;
      }
    }

    // Generate code based on event type
    if (event.type === "click") {
      const playwrightSelector = convertToPlaywrightSelector(event.selector);
      const comment = `Click ${index + 1}: ${playwrightSelector}`;

      actionsCode += `  
    // ${comment}
    const element${index + 1} = page.locator('${playwrightSelector}');
    await element${index + 1}.waitFor({ state: 'visible' });
    await element${index + 1}.click();
  `;
    } else if (event.type === "input") {
      const playwrightSelector = convertToPlaywrightSelector(event.selector);
      if (!generatedSelectors.has(playwrightSelector)) {
        const comment = `Input ${index + 1}: Type "${
          event.value
        }" into ${playwrightSelector}`;

        actionsCode += `  
    // ${comment}
    await page.locator('${playwrightSelector}').fill('${event.value.replace(
          /'/g,
          "\\'"
        )}');
  `;

        generatedSelectors.add(playwrightSelector);
      }
    }

    previousTimestamp = event.timestamp;
  });

  return actionsCode;
}

/**
 * Converts CSS selector to Playwright-friendly selector
 * @param {string} cssSelector - CSS selector string
 * @returns {string} - Playwright selector
 */
export function convertToPlaywrightSelector(cssSelector) {
  // Handle data-testid attributes specially (Playwright best practice)
  if (cssSelector.includes('[data-testid="')) {
    const testIdMatch = cssSelector.match(/\[data-testid="([^"]+)"\]/);
    if (testIdMatch) {
      return `[data-testid="${testIdMatch[1]}"]`;
    }
  }

  // Clean up the selector for Playwright
  let selector = cssSelector;

  // Remove html>body> prefix as it's usually not needed
  selector = selector.replace(/^html>body>/, "");

  // Handle class combinations properly
  selector = selector.replace(/\.([^.#\[]+)/g, ".$1");

  // Handle ID selectors
  selector = selector.replace(/#([^.#\[]+)/g, "#$1");

  return selector;
}

/**
 * Generates an empty test template
 * @returns {string} - Empty test template
 */
function generateEmptyTest(url) {
  return `import { test, expect } from '@playwright/test';
  
  test('User interaction replay', async ({ page }) => {
    // Navigate to the page
    await page.goto('${url || "http://localhost:3000/frame.html"}');
    
    // Wait for page to load
    await page.waitForLoadState('networkidle');
    
    // No interactions were recorded
    console.log('No user interactions to replay');
  });`;
}

// Browser compatibility (for non-module environments)
if (typeof window !== "undefined") {
  window.PlaywrightGenerator = {
    generatePlaywrightTest,
    convertToPlaywrightSelector,
  };
}
