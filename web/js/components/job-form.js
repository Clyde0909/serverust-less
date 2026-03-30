/**
 * Job Form Component
 * Handles creating and editing jobs
 */

const JobForm = {
    modal: null,
    form: null,
    isSaving: false,
    codeEditor: null,

    init() {
        this.modal = document.getElementById('job-modal');
        this.form = document.getElementById('job-form');
        this._initCodeEditor();
        this.setupFormHandlers();
    },

    _initCodeEditor() {
        const wrapper = document.getElementById('job-code-editor');
        if (!wrapper || this.codeEditor) return;
        this.codeEditor = CodeMirror(wrapper, {
            mode: 'python',
            theme: 'dracula',
            lineNumbers: true,
            indentUnit: 4,
            tabSize: 4,
            indentWithTabs: false,
            lineWrapping: false,
            autofocus: false,
            extraKeys: {
                Tab: (cm) => {
                    if (cm.somethingSelected()) {
                        cm.indentSelection('add');
                    } else {
                        cm.replaceSelection('    ', 'end');
                    }
                },
                'Shift-Tab': (cm) => cm.indentSelection('subtract'),
            },
        });
    },

    async _populateVenvSelect(selectedVenvId) {
        const sel = document.getElementById('job-venv');
        if (!sel) return;
        try {
            const response = await API.Venvs.list();
            const venvs = response.venvs || response;
            // Keep the default "main" option then add custom venvs
            sel.innerHTML = '<option value="">— main (default) —</option>' +
                venvs
                    .filter(v => v.venv_type !== 'main')
                    .map(v => {
                        const parts = (v.path || '').replace(/\\/g, '/').split('/');
                        const label = parts[parts.length - 1] || v.id;
                        const selected = v.id === selectedVenvId ? ' selected' : '';
                        return `<option value="${escapeHtml(v.id)}"${selected}>${escapeHtml(label)}</option>`;
                    })
                    .join('');
            if (selectedVenvId) sel.value = selectedVenvId;
        } catch (e) {
            Logger.error('Failed to load venvs for job form:', e);
        }
    },

    setupFormHandlers() {
        this.form?.addEventListener('submit', async (e) => {
            e.preventDefault();
            await this.save();
        });
    },

    openNew() {
        this.reset();
        document.getElementById('job-modal-title').textContent = 'New Job';
        document.getElementById('btn-save-job').textContent = 'Create Job';
        this._populateVenvSelect(null);
        Modal.open('job-modal');
    },

    async openEdit(jobId) {
        this.reset();
        try {
            const job = await API.Jobs.get(jobId);
            this.populateForm(job);
            document.getElementById('job-modal-title').textContent = 'Edit Job';
            document.getElementById('btn-save-job').textContent = 'Update Job';
            await this._populateVenvSelect(job.venv_id || null);
            Modal.open('job-modal');
        } catch (error) {
            Toast.error('Failed to load job', error.message);
        }
    },

    reset() {
        this.isSaving = false;
        this.form?.reset();
        document.getElementById('job-id').value = '';

        // Reset default values
        document.getElementById('job-timeout').value = '30';
        document.getElementById('job-memory').value = '128';
        document.getElementById('job-retries').value = '0';
        document.getElementById('job-priority').value = '0';

        // Reset venv select to default
        const sel = document.getElementById('job-venv');
        if (sel) sel.innerHTML = '<option value="">— main (default) —</option>';

        // Reset code editor
        if (this.codeEditor) {
            this.codeEditor.setValue("# Enter your Python code here\nprint('Hello, World!')");
            this.codeEditor.clearHistory();
            setTimeout(() => this.codeEditor.refresh(), 10);
        }
    },

    populateForm(job) {
        document.getElementById('job-id').value = job.id;
        document.getElementById('job-name').value = job.name;
        document.getElementById('job-description').value = job.description || '';
        document.getElementById('job-code').value = job.python_code;
        if (this.codeEditor) {
            this.codeEditor.setValue(job.python_code || '');
            this.codeEditor.clearHistory();
            setTimeout(() => this.codeEditor.refresh(), 10);
        }
        document.getElementById('job-timeout').value = job.timeout_seconds;
        document.getElementById('job-memory').value = job.memory_limit_mb;
        document.getElementById('job-retries').value = job.max_retries;
        document.getElementById('job-priority').value = job.priority;
    },

    async save() {
        if (this.isSaving) return;
        this.isSaving = true;

        const id = document.getElementById('job-id').value;
        const isNew = !id;

        const venvId = document.getElementById('job-venv')?.value || '';

        const jobData = {
            name: document.getElementById('job-name').value.trim(),
            description: document.getElementById('job-description').value.trim() || null,
            python_code: this.codeEditor ? this.codeEditor.getValue() : document.getElementById('job-code').value,
            timeout_seconds: parseInt(document.getElementById('job-timeout').value, 10),
            memory_limit_mb: parseInt(document.getElementById('job-memory').value, 10),
            max_retries: parseInt(document.getElementById('job-retries').value, 10),
            priority: parseInt(document.getElementById('job-priority').value, 10),
            venv_id: venvId || null,
        };

        if (!jobData.name) {
            Toast.error('Validation Error', 'Job name is required');
            document.getElementById('job-name').focus();
            this.isSaving = false;
            return;
        }
        if (!jobData.python_code) {
            Toast.error('Validation Error', 'Python code is required');
            if (this.codeEditor) this.codeEditor.focus();
            else document.getElementById('job-code').focus();
            this.isSaving = false;
            return;
        }
        if (jobData.timeout_seconds < 1 || jobData.timeout_seconds > 3600) {
            Toast.error('Validation Error', 'Timeout must be between 1 and 3600 seconds');
            this.isSaving = false;
            return;
        }
        if (jobData.memory_limit_mb < 16 || jobData.memory_limit_mb > 4096) {
            Toast.error('Validation Error', 'Memory limit must be between 16 and 4096 MB');
            this.isSaving = false;
            return;
        }

        const saveBtn = document.getElementById('btn-save-job');
        const originalText = saveBtn?.textContent;
        if (saveBtn) {
            saveBtn.disabled = true;
            saveBtn.textContent = isNew ? 'Creating…' : 'Updating…';
        }

        try {
            if (isNew) {
                await API.Jobs.create(jobData);
                Toast.success('Job created', `"${jobData.name}" has been created`);
            } else {
                await API.Jobs.update(id, jobData);
                Toast.success('Job updated', `"${jobData.name}" has been updated`);
            }
            Modal.close('job-modal');
            JobList.load();
        } catch (error) {
            Toast.error('Failed to save', error.message);
        } finally {
            this.isSaving = false;
            if (saveBtn) {
                saveBtn.disabled = false;
                saveBtn.textContent = originalText;
            }
        }
    },
};

// Initialize on DOM ready
document.addEventListener('DOMContentLoaded', () => {
    JobForm.init();
});
