import { answered, DISMISSED } from '@decryp7/paddock-ui/shell';
import { confirmAction, confirmInput } from './confirm';

describe('confirmAction (the mapping onto the shared pd.confirm)', () => {
  it('maps title, body and the danger flag onto heading, message and a destructive affirmative', () => {
    expect(confirmInput({ title: 'Remove x?', body: 'It goes away.', confirm: 'Remove it', danger: true })).toEqual({
      heading: 'Remove x?',
      message: 'It goes away.',
      confirm: { label: 'Remove it', destructive: true },
    });
  });

  it('is not destructive unless asked, and lists details only when there are some', () => {
    expect(confirmInput({ title: 't', body: 'b', confirm: 'Go' })).toEqual({ heading: 't', message: 'b', confirm: { label: 'Go', destructive: false } });
    expect(confirmInput({ title: 't', body: 'b', confirm: 'Go', details: ['one'] }).details).toEqual(['one']);
  });

  it('opens pd.confirm; only a confirmed answer is true', async () => {
    const openDialog = vi.fn().mockResolvedValueOnce(answered(true)).mockResolvedValueOnce(DISMISSED);
    const data = { title: 'Remove x?', body: 'b', confirm: 'Remove it' };
    expect(await confirmAction({ openDialog }, data)).toBe(true);
    expect(openDialog).toHaveBeenCalledWith('pd.confirm', confirmInput(data));
    expect(await confirmAction({ openDialog }, data)).toBe(false);
  });
});
