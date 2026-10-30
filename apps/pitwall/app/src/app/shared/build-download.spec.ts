import { buildDownloadName } from './build-download';

describe('buildDownloadName', () => {
  it('names a build as the server does', () => {
    expect(buildDownloadName('llama.cpp', { label: 'CUDA 13 (NVIDIA)', tag: 'b11702' })).toBe('llama.cpp CUDA 13 (NVIDIA) (b11702)');
  });
});
