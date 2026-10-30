import { deepActiveElement, isEditableElement, preserveSelection } from './selection';

describe('preserveSelection', () => {
  let p: HTMLElement;
  let other: HTMLElement;
  let input: HTMLInputElement;
  const select = (el: Element) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    getSelection()!.removeAllRanges();
    getSelection()!.addRange(r);
  };

  beforeEach(() => {
    p = document.createElement('p');
    p.textContent = 'keep me selected';
    other = document.createElement('p');
    other.textContent = 'something else';
    input = document.createElement('input');
    document.body.append(p, other, input);
  });
  afterEach(() => {
    getSelection()?.removeAllRanges();
    p.remove();
    other.remove();
    input.remove();
  });

  it('gives back a selection that was dropped (what WebKit does when a menu takes the focus)', () => {
    select(p);
    const saved = preserveSelection();
    getSelection()!.removeAllRanges();
    saved.restore();
    expect(getSelection()!.toString()).toBe('keep me selected');
  });

  it('never takes the caret from a focused text field', () => {
    select(p);
    const saved = preserveSelection();
    input.focus();
    getSelection()!.collapse(input, 0);
    saved.restore();
    expect(document.activeElement).toBe(input);
    expect(getSelection()!.toString()).toBe('');
  });

  it('keeps what the user selected meanwhile, saves no caret, and skips text that left the page', () => {
    select(p);
    const saved = preserveSelection();
    select(other);
    saved.restore();
    expect(getSelection()!.toString()).toBe('something else');

    getSelection()!.collapse(p.firstChild!, 2);
    const caret = preserveSelection();
    getSelection()!.removeAllRanges();
    caret.restore();
    expect(getSelection()!.rangeCount).toBe(0);

    select(p);
    const gone = preserveSelection();
    getSelection()!.removeAllRanges();
    p.remove();
    gone.restore();
    expect(getSelection()!.rangeCount).toBe(0);
  });

  it('never takes the caret from a text field inside a shadow root', () => {
    const host = document.createElement('div');
    document.body.append(host);
    const inner = document.createElement('input');
    host.attachShadow({ mode: 'open' }).append(inner);
    try {
      select(p);
      const saved = preserveSelection();
      inner.focus();
      getSelection()!.removeAllRanges();
      saved.restore();
      expect(getSelection()!.toString()).toBe('');
      expect(deepActiveElement(document)).toBe(inner);
    } finally {
      host.remove();
    }
  });

  it('gives the selection back when a focused input takes no text (a checkbox)', () => {
    const box = document.createElement('input');
    box.type = 'checkbox';
    document.body.append(box);
    try {
      select(p);
      const saved = preserveSelection();
      box.focus();
      getSelection()!.removeAllRanges();
      saved.restore();
      expect(getSelection()!.toString()).toBe('keep me selected');
    } finally {
      box.remove();
    }
  });

  it('knows the editable elements', () => {
    const div = document.createElement('div');
    expect(isEditableElement(null)).toBe(false);
    expect(isEditableElement(div)).toBe(false);
    expect(isEditableElement(input)).toBe(true);
    for (const type of ['search', 'email', 'url', 'tel', 'password', 'number', 'date', 'datetime-local', 'month', 'time', 'week', 'TEXT']) {
      const typed = document.createElement('input');
      typed.setAttribute('type', type);
      expect(isEditableElement(typed)).toBe(true);
    }
    for (const type of ['checkbox', 'radio', 'button', 'submit', 'range', 'color', 'file']) {
      const typed = document.createElement('input');
      typed.setAttribute('type', type);
      expect(isEditableElement(typed)).toBe(false);
    }
    expect(isEditableElement(document.createElement('textarea'))).toBe(true);
    expect(isEditableElement(document.createElement('select'))).toBe(true);
    div.setAttribute('contenteditable', 'true');
    expect(isEditableElement(div)).toBe(true);
    div.setAttribute('contenteditable', '');
    expect(isEditableElement(div)).toBe(true);
    expect(isEditableElement(document.createElementNS('http://www.w3.org/2000/svg', 'svg'))).toBe(false);
  });
});
