// @ts-nocheck
/**
 * FastLED UI Help Renderer Module
 *
 * Markdown conversion plus help-button, tooltip, and popup rendering helpers
 * extracted from `ui_manager.ts` so the main file can focus on the
 * `JsonUiManager` class.
 *
 * Exports:
 * - `markdownToHtml(markdown)` — minimal markdown -> HTML converter
 * - `createHelp(element)` — help-button DOM element with tooltip + popup wiring
 * - `showHelpPopup(htmlContent)` — modal popup for the rendered help content
 * - `showTooltip(button, tooltip)` / `hideTooltip(tooltip)` — tooltip helpers
 *
 * Behavior is preserved exactly as it was inside `ui_manager.ts`.
 *
 * @module UIHelpRenderer
 */

/* eslint-disable no-console */
/* eslint-disable no-restricted-syntax */
/* eslint-disable max-len */
/* eslint-disable guard-for-in */

/**
 * Simple markdown to HTML converter
 * Supports: headers, bold, italic, code, links, lists, and paragraphs
 * @param {string} markdown - Markdown text to convert
 * @returns {string} HTML string with converted markdown
 */
export function markdownToHtml(markdown) {
  if (!markdown) return '';

  let html = markdown;

  // Convert headers (# ## ### etc.)
  html = html.replace(/^### (.+)$/gm, '<h3>$1</h3>');
  html = html.replace(/^## (.+)$/gm, '<h2>$1</h2>');
  html = html.replace(/^# (.+)$/gm, '<h1>$1</h1>');

  // Convert bold **text** and __text__
  html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/__(.+?)__/g, '<strong>$1</strong>');

  // Convert italic *text* and _text_
  html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');
  html = html.replace(/_(.+?)_/g, '<em>$1</em>');

  // Convert inline code `code`
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>');

  // Convert code blocks ```code```
  html = html.replace(/```([^`]+)```/g, '<pre><code>$1</code></pre>');

  // Convert links [text](url)
  html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank">$1</a>');

  // Convert unordered lists (- item or * item)
  html = html.replace(/^[\-\*] (.+)$/gm, '<li>$1</li>');

  // Convert ordered lists (1. item, 2. item, etc.)
  html = html.replace(/^\d+\. (.+)$/gm, '<li class="ordered">$1</li>');

  // Wrap consecutive <li> elements properly
  html = html.replace(
    /(<li(?:\s+class="ordered")?>.*?<\/li>(?:\s*<li(?:\s+class="ordered")?>.*?<\/li>)*)/gs,
    (match) => {
      if (match.includes('class="ordered"')) {
        return `<ol>${match.replace(/\s+class="ordered"/g, '')}</ol>`;
      }
      return `<ul>${match}</ul>`;
    },
  );

  // Convert line breaks to paragraphs (double newlines become paragraph breaks)
  const paragraphs = html.split(/\n\s*\n/);
  html = paragraphs.map((p) => {
    const trimmed = p.trim();
    if (
      trimmed && !trimmed.startsWith('<h') && !trimmed.startsWith('<ul')
      && !trimmed.startsWith('<ol') && !trimmed.startsWith('<pre')
    ) {
      return `<p>${trimmed.replace(/\n/g, '<br>')}</p>`;
    }
    return trimmed;
  }).join('\n');

  return html;
}

export function createHelp(element) {
  const helpContainer = document.createElement('div');
  helpContainer.className = 'ui-help-container';
  helpContainer.id = `help-${element.id}`;

  // Create help button
  const helpButton = document.createElement('button');
  helpButton.className = 'ui-help-button';
  helpButton.textContent = '?';
  helpButton.setAttribute('type', 'button');
  helpButton.setAttribute('aria-label', 'Help');

  // Prepare content for tooltip and popup
  const markdownContent = element.markdownContent || '';
  const tooltipText = markdownContent.length > 200
    ? `${markdownContent.substring(0, 200).trim()}...`
    : markdownContent;

  // Convert markdown to HTML for popup
  const htmlContent = markdownToHtml(markdownContent);

  // Create tooltip
  const tooltip = document.createElement('div');
  tooltip.className = 'ui-help-tooltip';
  tooltip.textContent = tooltipText;

  // Add event listeners for tooltip
  helpButton.addEventListener('mouseenter', () => {
    if (tooltipText.trim()) {
      showTooltip(helpButton, tooltip);
    }
  });

  helpButton.addEventListener('mouseleave', () => {
    hideTooltip(tooltip);
  });

  // Add event listener for popup
  helpButton.addEventListener('click', (e) => {
    e.preventDefault();
    e.stopPropagation();
    showHelpPopup(htmlContent);
  });

  // Assemble the help container
  helpContainer.appendChild(helpButton);

  // Append tooltip to document body so it can appear above everything
  document.body.appendChild(tooltip);

  // Add styles if not already present
  if (!document.querySelector('#ui-help-styles')) {
    const style = document.createElement('style');
    style.id = 'ui-help-styles';
    style.textContent = `
      .ui-help-container {
        position: relative;
        display: inline-block;
        margin: 5px;
      }

      .ui-help-button {
        width: 24px;
        height: 24px;
        border-radius: 50%;
        background-color: #6c757d;
        color: white;
        border: none;
        font-size: 14px;
        font-weight: bold;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: background-color 0.2s ease;
      }

      .ui-help-button:hover {
        background-color: #5a6268;
      }

      .ui-help-tooltip {
        position: fixed;
        background-color: #333;
        color: white;
        padding: 8px 12px;
        border-radius: 4px;
        font-size: 16px;
        white-space: pre-wrap;
        max-width: 300px;
        z-index: 10001;
        visibility: hidden;
        opacity: 0;
        transition: opacity 0.2s ease;
        box-shadow: 0 2px 8px rgba(0,0,0,0.2);
        pointer-events: none;
      }



      .ui-help-popup {
        position: fixed;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        background-color: rgba(0, 0, 0, 0.8);
        z-index: 10000;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 20px;
        box-sizing: border-box;
      }

      .ui-help-popup-content {
        background-color: #2d3748;
        color: #e2e8f0;
        border-radius: 8px;
        max-width: 90%;
        max-height: 90%;
        overflow-y: auto;
        padding: 24px;
        position: relative;
        box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1), 0 10px 10px -5px rgba(0, 0, 0, 0.04);
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        line-height: 1.6;
      }

      .ui-help-popup-close {
        position: absolute;
        top: 16px;
        right: 16px;
        background: none;
        border: none;
        color: #a0aec0;
        font-size: 24px;
        cursor: pointer;
        padding: 4px;
        line-height: 1;
      }

      .ui-help-popup-close:hover {
        color: #e2e8f0;
      }

      .ui-help-popup-content h1,
      .ui-help-popup-content h2,
      .ui-help-popup-content h3 {
        color: #f7fafc;
        margin-top: 0;
        margin-bottom: 16px;
      }

      .ui-help-popup-content h1 { font-size: 1.875rem; }
      .ui-help-popup-content h2 { font-size: 1.5rem; }
      .ui-help-popup-content h3 { font-size: 1.25rem; }

      .ui-help-popup-content p {
        margin: 16px 0;
        color: #cbd5e0;
      }

      .ui-help-popup-content ul,
      .ui-help-popup-content ol {
        padding-left: 24px;
        margin: 16px 0;
      }

      .ui-help-popup-content li {
        margin: 8px 0;
        color: #cbd5e0;
      }

      .ui-help-popup-content code {
        background-color: #4a5568;
        color: #f7fafc;
        padding: 2px 6px;
        border-radius: 4px;
        font-family: 'JetBrains Mono', 'Fira Code', 'Courier New', monospace;
        font-size: 0.875rem;
      }

      .ui-help-popup-content pre {
        background-color: #1a202c;
        color: #f7fafc;
        padding: 16px;
        border-radius: 6px;
        overflow-x: auto;
        margin: 16px 0;
        border: 1px solid #4a5568;
      }

      .ui-help-popup-content pre code {
        background-color: transparent;
        padding: 0;
        color: inherit;
      }

      .ui-help-popup-content a {
        color: #63b3ed;
        text-decoration: none;
      }

      .ui-help-popup-content a:hover {
        color: #90cdf4;
        text-decoration: underline;
      }

      .ui-help-popup-content strong {
        color: #f7fafc;
        font-weight: 600;
      }

      .ui-help-popup-content em {
        color: #e2e8f0;
        font-style: italic;
      }

      .ui-help-popup-content blockquote {
        border-left: 4px solid #4a5568;
        margin: 16px 0;
        padding-left: 16px;
        color: #a0aec0;
        font-style: italic;
      }
    `;
    document.head.appendChild(style);
  }

  return helpContainer;
}

export function showHelpPopup(htmlContent) {
  // Remove any existing popup
  const existingPopup = document.querySelector('.ui-help-popup');
  if (existingPopup) {
    existingPopup.remove();
  }

  // Create popup
  const popup = document.createElement('div');
  popup.className = 'ui-help-popup';

  const popupContent = document.createElement('div');
  popupContent.className = 'ui-help-popup-content';

  const closeButton = document.createElement('button');
  closeButton.className = 'ui-help-popup-close';
  closeButton.innerHTML = '&times;';
  closeButton.setAttribute('aria-label', 'Close help');

  const contentDiv = document.createElement('div');
  contentDiv.innerHTML = htmlContent;

  popupContent.appendChild(closeButton);
  popupContent.appendChild(contentDiv);
  popup.appendChild(popupContent);

  // Add event listeners
  closeButton.addEventListener('click', () => {
    popup.remove();
  });

  popup.addEventListener('click', (e) => {
    if (e.target === popup) {
      popup.remove();
    }
  });

  // Handle escape key
  const handleEscape = (e) => {
    if (e.key === 'Escape') {
      popup.remove();
      document.removeEventListener('keydown', handleEscape);
    }
  };
  document.addEventListener('keydown', handleEscape);

  // Add to DOM
  document.body.appendChild(popup);
}

export function showTooltip(button, tooltip) {
  // Get button position relative to viewport
  const buttonRect = button.getBoundingClientRect();

  // Make tooltip visible but transparent to measure it
  tooltip.style.visibility = 'visible';
  tooltip.style.opacity = '0';

  // Now get tooltip dimensions
  const tooltipRect = tooltip.getBoundingClientRect();

  // Calculate tooltip position
  const buttonCenterX = buttonRect.left + buttonRect.width / 2;
  const tooltipTop = buttonRect.top - tooltipRect.height - 8; // 8px gap above button

  // Position tooltip centered above the button
  let tooltipLeft = buttonCenterX - tooltipRect.width / 2;

  // Ensure tooltip doesn't go off-screen horizontally
  const padding = 10;
  if (tooltipLeft < padding) {
    tooltipLeft = padding;
  } else if (tooltipLeft + tooltipRect.width > window.innerWidth - padding) {
    tooltipLeft = window.innerWidth - tooltipRect.width - padding;
  }

  // Position tooltip
  tooltip.style.left = `${tooltipLeft}px`;
  tooltip.style.top = `${tooltipTop}px`;

  // Show tooltip with fade-in
  tooltip.style.opacity = '1';
}

export function hideTooltip(tooltip) {
  tooltip.style.visibility = 'hidden';
  tooltip.style.opacity = '0';
}
