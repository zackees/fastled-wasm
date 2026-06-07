// @ts-nocheck
/**
 * FastLED UI Control Factory Module
 *
 * Pure DOM control builders used by `JsonUiManager`. These were extracted from
 * `ui_manager.ts` to keep that file focused on the manager class itself.
 *
 * Exports:
 * - `groupAdjacentNumberFields(elements)` — adjacency-based pairing helper
 * - `createNumberField(element)` / `createNumberFieldPair(...)` /
 *   `createSingleNumberField(element)` — number input builders
 * - `createSlider(element)` / `createCheckbox(element)` / `createButton(element)`
 *   / `createDropdown(element)` — basic controls
 * - `setTitle(titleData)` / `setDescription(descData)` — page-level metadata
 *
 * Behavior is preserved exactly as it was inside `ui_manager.ts`.
 *
 * @module UIControlFactory
 */

/* eslint-disable no-console */
/* eslint-disable no-restricted-syntax */
/* eslint-disable max-len */
/* eslint-disable guard-for-in */

import { markdownToHtml } from './ui_help_renderer.ts';

/**
 * Groups adjacent number fields into pairs for more efficient space utilization
 * @param {Array} elements - Array of UI element configurations
 * @returns {Array} Array with adjacent number fields grouped into pairs
 */
export function groupAdjacentNumberFields(elements) {
  const result = [];
  let i = 0;

  while (i < elements.length) {
    const current = elements[i];

    // Check if current element is a number field and the next one is also a number field
    if (current.type === 'number' && i + 1 < elements.length && elements[i + 1].type === 'number') {
      const next = elements[i + 1];

      // Create a paired element that will be handled specially
      result.push({
        type: 'number-pair',
        leftElement: current,
        rightElement: next,
        id: `pair-${current.id}-${next.id}`,
        group: current.group, // Use the group from the first element
      });

      i += 2; // Skip both elements since we've paired them
    } else {
      // Add single element as-is
      result.push(current);
      i += 1;
    }
  }

  return result;
}

/**
 * Creates a number input field UI element
 * @param {Object} element - Element configuration object
 * @param {string} element.name - Display name for the input
 * @param {string} element.id - Unique identifier for the input
 * @param {number} element.value - Current/default value
 * @param {number} element.min - Minimum allowed value
 * @param {number} element.max - Maximum allowed value
 * @param {number} [element.step] - Step increment (defaults to 'any')
 * @returns {HTMLDivElement} Container div with label and number input
 */
export function createNumberField(element) {
  return createSingleNumberField(element);
}

/**
 * Creates a paired number input field UI element with two number fields side by side
 * @param {Object} leftElement - Left element configuration object
 * @param {Object} rightElement - Right element configuration object
 * @returns {HTMLDivElement} Container div with two number fields in a flex layout
 */
export function createNumberFieldPair(leftElement, rightElement) {
  const pairContainer = document.createElement('div');
  pairContainer.className = 'ui-control number-pair-control';
  pairContainer.style.display = 'flex';
  pairContainer.style.gap = '20px';
  pairContainer.style.alignItems = 'center';
  pairContainer.style.justifyContent = 'space-between';

  // Create left number field
  const leftField = createSingleNumberField(leftElement);
  leftField.style.flex = '1';
  leftField.style.maxWidth = 'calc(50% - 10px)';

  // Create right number field
  const rightField = createSingleNumberField(rightElement);
  rightField.style.flex = '1';
  rightField.style.maxWidth = 'calc(50% - 10px)';

  pairContainer.appendChild(leftField);
  pairContainer.appendChild(rightField);

  // Store reference to both elements for later registration
  /** @type {HTMLDivElement & {_leftElement?: any, _rightElement?: any, _leftControl?: any, _rightControl?: any}} */
  const containerWithProps = /** @type {any} */ (pairContainer);
  containerWithProps._leftElement = leftElement;
  containerWithProps._rightElement = rightElement;
  containerWithProps._leftControl = leftField;
  containerWithProps._rightControl = rightField;

  return pairContainer;
}

/**
 * Creates a single number input field UI element (used by both single and paired fields)
 * @param {Object} element - Element configuration object
 * @returns {HTMLDivElement} Container div with label and number input
 */
export function createSingleNumberField(element) {
  const controlDiv = document.createElement('div');
  controlDiv.className = 'ui-control number-control inline-row single-number-field';

  const label = document.createElement('label');
  label.textContent = element.name;
  label.htmlFor = `number-${element.id}`;
  label.style.display = 'inline-block';
  label.style.verticalAlign = 'middle';
  label.style.fontWeight = '500';
  label.style.color = '#E0E0E0';
  label.style.marginRight = '10px';
  label.style.fontSize = '0.9em'; // Slightly smaller for paired layout

  const numberInput = document.createElement('input');
  numberInput.type = 'number';
  numberInput.id = `number-${element.id}`;
  numberInput.value = element.value;
  numberInput.min = element.min;
  numberInput.max = element.max;
  numberInput.step = (element.step !== undefined) ? element.step : 'any';
  numberInput.style.display = 'inline-block';
  numberInput.style.verticalAlign = 'middle';
  numberInput.style.boxSizing = 'border-box';

  // Calculate dynamic width based on expected value range
  const calculateInputWidth = () => {
    const minDigits = String(Math.floor(element.min || 0)).length;
    const maxDigits = String(Math.floor(element.max || 0)).length;
    const valueDigits = String(Math.floor(element.value || 0)).length;
    const maxLength = Math.max(minDigits, maxDigits, valueDigits, 5); // Minimum 5 digits
    // ~8px per character + 20px for padding/controls
    return `${maxLength * 8 + 20}px`;
  };

  numberInput.style.width = calculateInputWidth();
  numberInput.style.minWidth = '70px'; // Ensure minimum usable width

  controlDiv.appendChild(label);
  controlDiv.appendChild(numberInput);

  return controlDiv;
}

/**
 * Creates a slider (range) input UI element with live value display
 * @param {Object} element - Element configuration object
 * @param {string} element.name - Display name for the slider
 * @param {string} element.id - Unique identifier for the slider
 * @param {number} element.value - Current/default value
 * @param {number} element.min - Minimum allowed value
 * @param {number} element.max - Maximum allowed value
 * @param {number} element.step - Step increment for the slider
 * @returns {HTMLDivElement} Container div with label, value display, and slider
 */
export function createSlider(element) {
  const controlDiv = document.createElement('div');
  controlDiv.className = 'ui-control slider-control';

  // Create the slider container with relative positioning
  const sliderContainer = document.createElement('div');
  sliderContainer.className = 'slider-container';

  // Create the slider input
  const slider = document.createElement('input');
  slider.type = 'range';
  slider.id = `slider-${element.id}`;
  slider.setAttribute('min', String(Number.parseFloat(String(element.min))));
  slider.setAttribute('max', String(Number.parseFloat(String(element.max))));
  slider.setAttribute('value', String(Number.parseFloat(String(element.value))));

  // Check if element.step exists and is not undefined/null
  if (element.step !== undefined && element.step !== null) {
    slider.setAttribute('step', String(Number.parseFloat(String(element.step))));
  } else {
    // Set a default step
    slider.step = 'any';
  }

  // Create the overlay label div
  const overlayDiv = document.createElement('div');
  overlayDiv.className = 'slider-label-overlay';

  const labelText = document.createElement('span');
  labelText.className = 'label-text';
  labelText.textContent = element.name;

  const valueDisplay = document.createElement('span');
  valueDisplay.className = 'slider-value';
  valueDisplay.textContent = Number(element.value).toFixed(3);

  overlayDiv.appendChild(labelText);
  overlayDiv.appendChild(valueDisplay);

  // Set initial value in next frame to ensure proper initialization
  setTimeout(() => {
    slider.setAttribute('value', String(Number.parseFloat(String(element.value))));
    valueDisplay.textContent = Number(slider.value).toFixed(3);
  }, 0);

  // Update value display when slider changes
  slider.addEventListener('input', () => {
    valueDisplay.textContent = Number(slider.value).toFixed(3);
  });

  // Add elements to container
  sliderContainer.appendChild(slider);
  sliderContainer.appendChild(overlayDiv);
  controlDiv.appendChild(sliderContainer);

  return controlDiv;
}

/**
 * Creates a checkbox UI element for boolean values
 * @param {Object} element - Element configuration object
 * @param {string} element.name - Display name for the checkbox
 * @param {string} element.id - Unique identifier for the checkbox
 * @param {boolean} element.value - Current/default checked state
 * @returns {HTMLDivElement} Container div with label and checkbox
 */
export function createCheckbox(element) {
  const controlDiv = document.createElement('div');
  controlDiv.className = 'ui-control';

  const label = document.createElement('label');
  label.textContent = element.name;
  label.htmlFor = `checkbox-${element.id}`;

  const checkbox = document.createElement('input');
  checkbox.type = 'checkbox';
  checkbox.id = `checkbox-${element.id}`;
  checkbox.checked = element.value;

  const flexContainer = document.createElement('div');
  flexContainer.style.display = 'flex';
  flexContainer.style.alignItems = 'center';
  flexContainer.style.justifyContent = 'space-between';

  flexContainer.appendChild(label);
  flexContainer.appendChild(checkbox);

  controlDiv.appendChild(flexContainer);

  return controlDiv;
}

/**
 * Creates a button UI element with press/release state tracking
 * @param {Object} element - Element configuration object
 * @param {string} element.name - Display text for the button
 * @param {string} element.id - Unique identifier for the button
 * @returns {HTMLDivElement} Container div with configured button element
 */
export function createButton(element) {
  const controlDiv = document.createElement('div');
  controlDiv.className = 'ui-control';

  const button = document.createElement('button');
  button.textContent = element.name;
  button.id = `button-${element.id}`;
  button.setAttribute('data-pressed', 'false');

  button.addEventListener('mousedown', () => {
    button.setAttribute('data-pressed', 'true');
    button.classList.add('active');
  });

  button.addEventListener('mouseup', () => {
    button.setAttribute('data-pressed', 'false');
    button.classList.remove('active');
  });

  button.addEventListener('mouseleave', () => {
    button.setAttribute('data-pressed', 'false');
    button.classList.remove('active');
  });
  controlDiv.appendChild(button);
  return controlDiv;
}

export function createDropdown(element) {
  const controlDiv = document.createElement('div');
  controlDiv.className = 'ui-control';

  const label = document.createElement('label');
  label.textContent = element.name;
  label.htmlFor = `dropdown-${element.id}`;

  const dropdown = document.createElement('select');
  dropdown.id = `dropdown-${element.id}`;
  dropdown.value = element.value;

  // Add options to the dropdown
  if (element.options && Array.isArray(element.options)) {
    element.options.forEach((option, index) => {
      const optionElement = document.createElement('option');
      optionElement.value = index;
      optionElement.textContent = option;
      dropdown.appendChild(optionElement);
    });
  }

  // Set the selected option
  dropdown.selectedIndex = element.value;

  controlDiv.appendChild(label);
  controlDiv.appendChild(dropdown);

  return controlDiv;
}

export function setTitle(titleData) {
  if (titleData && titleData.text) {
    document.title = titleData.text;
    const h1Element = document.querySelector('h1');
    if (h1Element) {
      h1Element.textContent = titleData.text;
    } else {
      console.warn('H1 element not found in document');
    }
  } else {
    console.warn('Invalid title data received:', titleData);
  }
}

export function setDescription(descData) {
  if (descData && descData.text) {
    // Create or find description element
    let descElement = document.querySelector('#fastled-description');
    if (!descElement) {
      descElement = document.createElement('div');
      descElement.id = 'fastled-description';
      // Insert after h1
      const h1Element = document.querySelector('h1');
      if (h1Element && h1Element.nextSibling) {
        h1Element.parentNode.insertBefore(descElement, h1Element.nextSibling);
      } else {
        console.warn('Could not find h1 element to insert description after');
        document.body.insertBefore(descElement, document.body.firstChild);
      }
    }

    // Always process text as markdown (plain text is valid markdown)
    descElement.innerHTML = markdownToHtml(descData.text);
  } else {
    console.warn('Invalid description data received:', descData);
  }
}
