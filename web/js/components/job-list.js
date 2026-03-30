/**
 * Job List Component
 * Handles displaying and managing the jobs list
 */

const JobList = {
    container: null,
    emptyState: null,
    loadingState: null,
    allJobs: [],
    
    init() {
        this.container = document.getElementById('jobs-list');
        this.emptyState = document.getElementById('jobs-empty');
        this.loadingState = document.getElementById('jobs-loading');
    },
    
    async load(silent = false) {
        this.init();
        Logger.debug('JobList.load called, silent:', silent);
        
        if (!silent) {
            this.container.innerHTML = '';
            this.loadingState.classList.remove('hidden');
            this.emptyState.classList.add('hidden');
        }
        
        try {
            const [jobsResponse, venvsResponse] = await Promise.all([
                API.Jobs.list({ limit: 100 }),
                API.Venvs.list().catch(() => ({ venvs: [] })),
            ]);
            const jobs = jobsResponse.jobs || jobsResponse;
            const venvs = venvsResponse.venvs || venvsResponse || [];
            Logger.info('Loaded', jobs.length, 'jobs');
            this.allJobs = jobs;
            AppState.jobs = jobs;
            AppState.venvs = venvs;
            this.render(jobs);
        } catch (error) {
            Logger.error('Failed to load jobs:', error);
            Toast.error('Failed to load jobs', error.message);
            this.emptyState.classList.remove('hidden');
        } finally {
            this.loadingState.classList.add('hidden');
        }
    },
    
    filter(searchTerm) {
        if (!searchTerm) {
            this.render(this.allJobs);
            return;
        }
        
        const term = searchTerm.toLowerCase();
        const filtered = this.allJobs.filter(job => 
            job.name.toLowerCase().includes(term) ||
            (job.description && job.description.toLowerCase().includes(term))
        );
        this.render(filtered);
    },
    
    render(jobs) {
        this.init();
        
        if (!jobs || jobs.length === 0) {
            this.container.innerHTML = '';
            this.emptyState.classList.remove('hidden');
            return;
        }
        
        this.emptyState.classList.add('hidden');
        this.container.innerHTML = jobs.map(job => this.renderJobCard(job)).join('');
        
        // Attach event listeners
        this.container.querySelectorAll('[data-action]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                const action = e.currentTarget.dataset.action;
                const jobId = e.currentTarget.dataset.jobId;
                this.handleAction(action, jobId);
            });
        });
    },
    
    renderJobCard(job) {
        let venvLabel = 'main';
        let venvBadgeClass = 'badge-info';
        if (job.venv_id) {
            // Try to get venv name from state, fallback to truncated id
            const venvs = AppState.venvs || [];
            const venv = venvs.find(v => v.id === job.venv_id);
            if (venv && venv.path) {
                const parts = venv.path.replace(/\\/g, '/').split('/');
                venvLabel = parts[parts.length - 1] || job.venv_id;
            } else {
                venvLabel = job.venv_id.slice(0, 8);
            }
            venvBadgeClass = 'badge-warning';
        }

        return `
            <div class="card" data-job-id="${job.id}">
                <div class="card-header">
                    <h3 class="card-title">${escapeHtml(job.name)}</h3>
                    <span class="badge ${job.enabled ? 'badge-success' : 'badge-secondary'}">
                        ${job.enabled ? 'Enabled' : 'Disabled'}
                    </span>
                </div>
                ${job.description ? `<p class="card-description">${escapeHtml(job.description)}</p>` : ''}
                <div class="card-meta">
                    <span class="badge ${venvBadgeClass}" title="Virtual Environment">
                        🐍 ${escapeHtml(venvLabel)} venv
                    </span>
                    <span class="badge badge-secondary" title="Timeout">
                        ⏱️ ${job.timeout_seconds}s
                    </span>
                    <span class="badge badge-secondary" title="Memory Limit">
                        💾 ${job.memory_limit_mb}MB
                    </span>
                    ${job.priority !== 0 ? `
                        <span class="badge badge-secondary" title="Priority">
                            📊 Priority: ${job.priority}
                        </span>
                    ` : ''}
                </div>
                <div class="card-actions">
                    <div class="card-actions-primary">
                        <button class="btn btn-primary btn-sm" data-action="execute" data-job-id="${job.id}">
                            <span class="icon">▶️</span> Execute
                        </button>
                        <button class="btn ${job.enabled ? 'btn-secondary' : 'btn-success'} btn-sm" data-action="toggle-enable" data-job-id="${job.id}">
                            <span class="icon">${job.enabled ? '🔴' : '🟢'}</span> ${job.enabled ? 'Disable' : 'Enable'}
                        </button>
                    </div>
                    <div class="card-actions-secondary">
                        <button class="btn btn-secondary btn-icon" data-action="edit" data-job-id="${job.id}" title="Edit">
                            ✏️
                        </button>
                        <button class="btn btn-secondary btn-icon" data-action="view" data-job-id="${job.id}" title="View">
                            👁️
                        </button>
                        <button class="btn btn-danger btn-icon" data-action="delete" data-job-id="${job.id}" title="Delete">
                            🗑️
                        </button>
                    </div>
                </div>
            </div>
        `;
    },
    
    async handleAction(action, jobId) {
        switch (action) {
            case 'execute':
                await this.openExecuteModal(jobId);
                break;
            case 'toggle-enable':
                await this.toggleEnable(jobId);
                break;
            case 'edit':
                await JobForm.openEdit(jobId);
                break;
            case 'view':
                await this.viewJob(jobId);
                break;
            case 'delete':
                await this.deleteJob(jobId);
                break;
        }
    },
    
    async openExecuteModal(jobId) {
        const job = this.allJobs.find(j => j.id === jobId);
        if (!job) return;

        // Auto-enable disabled job before executing
        if (!job.enabled) {
            try {
                await API.Jobs.enable(jobId);
                job.enabled = true;
                Toast.info('Job enabled', `"${job.name}" has been enabled and will now run`);
                this.render(this.allJobs);
            } catch (error) {
                Toast.error('Failed to enable job', error.message);
                return;
            }
        }

        AppState.selectedJob = job;
        document.getElementById('execute-job-name').textContent = `Execute: ${job.name}`;
        document.getElementById('execute-input').value = '';
        document.getElementById('execute-priority').value = '';
        
        // Setup execute button
        const confirmBtn = document.getElementById('btn-confirm-execute');
        confirmBtn.onclick = async () => {
            await this.executeJob(jobId);
        };
        
        Modal.open('execute-modal');
    },
    
    async executeJob(jobId) {
        const inputEl = document.getElementById('execute-input');
        const priorityEl = document.getElementById('execute-priority');
        
        let inputData = null;
        if (inputEl.value.trim()) {
            try {
                inputData = JSON.parse(inputEl.value);
            } catch (e) {
                Toast.error('Invalid JSON', 'Input data must be valid JSON');
                return;
            }
        }
        
        const options = {};
        if (inputData) options.input_data = inputData;
        if (priorityEl.value) options.priority = parseInt(priorityEl.value, 10);
        
        try {
            const result = await API.Jobs.execute(jobId, options);
            Toast.success('Job queued', `Execution ID: ${result.execution_id || result.id}`);
            Modal.close('execute-modal');
            
            // Switch to executions view
            navigateTo('executions');
        } catch (error) {
            Toast.error('Execution failed', error.message);
        }
    },
    
    async toggleEnable(jobId) {
        const job = this.allJobs.find(j => j.id === jobId);
        if (!job) return;

        try {
            if (job.enabled) {
                await API.Jobs.disable(jobId);
                job.enabled = false;
                Toast.success('Job disabled', `"${job.name}" has been disabled`);
            } else {
                await API.Jobs.enable(jobId);
                job.enabled = true;
                Toast.success('Job enabled', `"${job.name}" has been enabled`);
            }
            this.render(this.allJobs);
        } catch (error) {
            Toast.error('Failed to update job', error.message);
        }
    },

    async viewJob(jobId) {
        try {
            const job = await API.Jobs.get(jobId);
            // For now, open in edit mode but could create a read-only view
            await JobForm.openEdit(jobId);
        } catch (error) {
            Toast.error('Failed to load job', error.message);
        }
    },
    
    async deleteJob(jobId) {
        const job = this.allJobs.find(j => j.id === jobId);
        if (!job) return;
        
        const ok = await Confirm.show(`Are you sure you want to delete "${job.name}"?`, { title: 'Delete Job', confirmText: 'Delete' });
        if (!ok) return;
        
        try {
            await API.Jobs.delete(jobId);
            Toast.success('Job deleted', `"${job.name}" has been deleted`);
            this.load();
        } catch (error) {
            Toast.error('Failed to delete', error.message);
        }
    },
};
