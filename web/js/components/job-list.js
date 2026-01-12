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
            const response = await API.Jobs.list({ limit: 100 });
            const jobs = response.jobs || response;
            Logger.info('Loaded', jobs.length, 'jobs');
            this.allJobs = jobs;
            AppState.jobs = jobs;
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
        const venvType = job.use_custom_venv ? 'custom' : 'main';
        const venvBadgeClass = job.use_custom_venv ? 'badge-warning' : 'badge-info';
        
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
                        🐍 ${venvType} venv
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
                    <button class="btn btn-primary btn-sm" data-action="execute" data-job-id="${job.id}">
                        <span class="icon">▶️</span> Execute
                    </button>
                    <button class="btn btn-secondary btn-sm" data-action="edit" data-job-id="${job.id}">
                        <span class="icon">✏️</span> Edit
                    </button>
                    <button class="btn btn-secondary btn-sm" data-action="view" data-job-id="${job.id}">
                        <span class="icon">👁️</span> View
                    </button>
                    <button class="btn btn-danger btn-sm" data-action="delete" data-job-id="${job.id}">
                        <span class="icon">🗑️</span>
                    </button>
                </div>
            </div>
        `;
    },
    
    async handleAction(action, jobId) {
        switch (action) {
            case 'execute':
                this.openExecuteModal(jobId);
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
    
    openExecuteModal(jobId) {
        const job = this.allJobs.find(j => j.id === jobId);
        if (!job) return;
        
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
        
        if (!confirm(`Are you sure you want to delete "${job.name}"?`)) {
            return;
        }
        
        try {
            await API.Jobs.delete(jobId);
            Toast.success('Job deleted', `"${job.name}" has been deleted`);
            this.load();
        } catch (error) {
            Toast.error('Failed to delete', error.message);
        }
    },
};
