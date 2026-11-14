import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import DocTemplatesEditor from '../DocTemplatesEditor';

const PID = 3;
const ok = (data: unknown) => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data });

function tpl(docType: string, source: 'builtin' | 'project') {
  return {
    docType,
    source,
    templateRef: `${docType}@1`,
    skeleton: `# ${docType}\n`,
    prompt: 'p',
    checklist: ['c'],
    path: `docs/changes/_templates/${docType}.md`,
    content: `---\nsymphony_template:\n  prompt: p\n---\n# ${docType}\n`,
  };
}

function renderEditor() {
  render(
    <ThemeProvider theme={theme}>
      <DocTemplatesEditor projectId={PID} />
    </ThemeProvider>,
  );
}

describe('DocTemplatesEditor', () => {
  let saved: unknown[];
  let trials: unknown[];
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    saved = [];
    trials = [];
    server.use(
      http.get(`*/api/projects/${PID}/doc-templates`, () =>
        ok([tpl('intent', 'builtin'), tpl('spec', 'builtin'), tpl('plan', 'project')]),
      ),
      http.put(`*/api/projects/${PID}/doc-templates/:doc`, async ({ request, params }) => {
        saved.push({ doc: params.doc, ...((await request.json()) as object) });
        return ok({ branch: 'symphony/doc-templates-1', mrIid: 8, mrUrl: 'http://gitlab.local/mr/8' });
      }),
      http.post(`*/api/projects/${PID}/doc-templates/:doc/trial`, async ({ request, params }) => {
        trials.push({ doc: params.doc, ...((await request.json()) as object) });
        return ok({ status: 'completed', error: null, content: '# Plan\n\n## Steps\n1. export\n' });
      }),
    );
  });

  it('shows each template with its effective source', async () => {
    const user = userEvent.setup();
    renderEditor();
    const editor = await screen.findByLabelText('模板内容');
    expect(editor).toHaveValue('---\nsymphony_template:\n  prompt: p\n---\n# intent\n');
    expect(screen.getByTestId('template-source')).toHaveTextContent('内置模板');
    await user.click(screen.getByRole('button', { name: 'plan' }));
    expect(screen.getByTestId('template-source')).toHaveTextContent('项目模板 · docs/changes/_templates/plan.md');
  });

  it('saves through an MR and links to it', async () => {
    const user = userEvent.setup();
    renderEditor();
    const editor = await screen.findByLabelText('模板内容');
    expect(screen.getByRole('button', { name: '保存（创建 MR）' })).toBeDisabled();
    await user.type(editor, 'extra');
    await user.click(screen.getByRole('button', { name: '保存（创建 MR）' }));
    const link = await screen.findByRole('link', { name: '!8' });
    expect(link).toHaveAttribute('href', 'http://gitlab.local/mr/8');
    expect(saved).toEqual([{ doc: 'intent', content: '---\nsymphony_template:\n  prompt: p\n---\n# intent\nextra' }]);
  });

  it('shows the server validation error on save', async () => {
    const user = userEvent.setup();
    server.use(
      http.put(`*/api/projects/${PID}/doc-templates/:doc`, () =>
        HttpResponse.json({ success: false, retCode: 'BIZ_001', retMsg: '骨架缺少必填章节 ## Steps' }, { status: 400 }),
      ),
    );
    renderEditor();
    await user.type(await screen.findByLabelText('模板内容'), 'x');
    await user.click(screen.getByRole('button', { name: '保存（创建 MR）' }));
    expect(await screen.findByText('骨架缺少必填章节 ## Steps')).toBeInTheDocument();
  });

  it('trial drafts with the edited template and previews the result', async () => {
    const user = userEvent.setup();
    renderEditor();
    await user.click(await screen.findByRole('button', { name: 'plan' }));
    const trialBtn = screen.getByRole('button', { name: '试生成' });
    expect(trialBtn).toBeDisabled();
    await user.type(screen.getByLabelText('示例需求'), '导出报表');
    await user.click(trialBtn);
    const result = await screen.findByTestId('trial-result');
    await waitFor(() => expect(within(result).getByText('export')).toBeInTheDocument());
    expect(trials).toEqual([
      { doc: 'plan', content: '---\nsymphony_template:\n  prompt: p\n---\n# plan\n', sample: '导出报表' },
    ]);
  });
});
