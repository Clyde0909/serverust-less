/**
 * Job Form Component
 * Handles creating and editing jobs
 */

const JobForm = {
    modal: null,
    form: null,
    dependencies: [],
    
    init() {
        this.modal = document.getElementById('job-modal');
        this.form = document.getElementById('job-form');
        this.setupFormHandlers();
    },
    
    setupFormHandlers() {
        // Form submission
        this.form?.addEventListener('submit', async (e) => {
            e.preventDefault();
            await this.save();
        });
        
        // Add dependency button
        document.getElementById('btn-add-dependency')?.addEventListener('click', () => {
            this.addDependency();
        });
        
        // Add dependency on Enter key
        document.getElementById('new-dep-name')?.addEventListener('keypress', (e) => {
            if (e.key === 'Enter') {
                e.preventDefault();
                this.addDependency();
            }
        });
        
        document.getElementById('new-dep-version')?.addEventListener('keypress', (e) => {
            if (e.key === 'Enter') {
                e.preventDefault();
                this.addDependency();
            }
        });
    },
    
    openNew() {
        this.init();
        this.reset();
        
        document.getElementById('job-modal-title').textContent = 'New Job';
        document.getElementById('btn-save-job').textContent = 'Create Job';
        
        Modal.open('job-modal');
    },
    
    async openEdit(jobId) {
        this.init();
        this.reset();
        
        try {
            const job = await API.Jobs.get(jobId);
            this.populateForm(job);
            
            document.getElementById('job-modal-title').textContent = 'Edit Job';
            document.getElementById('btn-save-job').textContent = 'Update Job';
            
            Modal.open('job-modal');
        } catch (error) {
            Toast.error('Failed to load job', error.message);
        }
    },
    
    reset() {
        this.form?.reset();
        document.getElementById('job-id').value = '';
        this.dependencies = [];
        this.renderDependencies();
        
        // Reset default values
        document.getElementById('job-timeout').value = '30';
        document.getElementById('job-memory').value = '128';
        document.getElementById('job-retries').value = '0';
        document.getElementById('job-priority').value = '0';
        document.getElementById('job-custom-venv').checked = false;
    },
    
    populateForm(job) {
        document.getElementById('job-id').value = job.id;
        document.getElementById('job-name').value = job.name;
        document.getElementById('job-description').value = job.description || '';
        document.getElementById('job-code').value = job.python_code;
        document.getElementById('job-timeout').value = job.timeout_seconds;
        document.getElementById('job-memory').value = job.memory_limit_mb;
        document.getElementById('job-retries').value = job.max_retries;
        document.getElementById('job-priority').value = job.priority;
        document.getElementById('job-custom-venv').checked = job.use_custom_venv;
        
        // Parse and set dependencies
        this.dependencies = [];
        if (job.dependencies) {
            try {
                const deps = JSON.parse(job.dependencies);
                if (Array.isArray(deps)) {
                    this.dependencies = deps.map(d => {
                        if (typeof d === 'string') {
                            const parts = d.split(/([<>=!]+)/);
                            return {
                                name: parts[0],
                                version: parts.slice(1).join('') || null
                            };
                        }
                        return d;
                    });
                }
            } catch (e) {
                console.error('Failed to parse dependencies:', e);
            }
        }
        this.renderDependencies();
    },
    
    addDependency() {
        const nameInput = document.getElementById('new-dep-name');
        const versionInput = document.getElementById('new-dep-version');
        
        const name = nameInput.value.trim();
        if (!name) {
            Toast.warning('Missing name', 'Package name is required');
            nameInput.focus();
            return;
        }
        
        // Check for duplicates
        if (this.dependencies.some(d => d.name.toLowerCase() === name.toLowerCase())) {
            Toast.warning('Duplicate', 'This package is already in the list');
            return;
        }
        
        this.dependencies.push({
            name: name,
            version: versionInput.value.trim() || null
        });
        
        nameInput.value = '';
        versionInput.value = '';
        nameInput.focus();
        
        this.renderDependencies();
    },
    
    removeDependency(index) {
        this.dependencies.splice(index, 1);
        this.renderDependencies();
    },
    
    renderDependencies() {
        const container = document.getElementById('dependencies-list');
        if (!container) return;
        
        if (this.dependencies.length === 0) {
            container.innerHTML = '<span style="color: var(--text-muted); font-size: 0.875rem;">No dependencies added</span>';
            return;
        }
        
        container.innerHTML = this.dependencies.map((dep, index) => `
            <span class="dependency-tag">
                <span>${escapeHtml(dep.name)}${dep.version ? escapeHtml(dep.version) : ''}</span>
                <button type="button" class="remove-dep" data-index="${index}">&times;</button>
            </span>
        `).join('');
        
        // Attach remove handlers
        container.querySelectorAll('.remove-dep').forEach(btn => {
            btn.addEventListener('click', (e) => {
                const index = parseInt(e.target.dataset.index, 10);
                this.removeDependency(index);
            });
        });
    },
    
    async save() {
        const id = document.getElementById('job-id').value;
        const isNew = !id;
        
        // Gather form data
        const jobData = {
            name: document.getElementById('job-name').value.trim(),
            description: document.getElementById('job-description').value.trim() || null,
            python_code: document.getElementById('job-code').value,
            timeout_seconds: parseInt(document.getElementById('job-timeout').value, 10),
            memory_limit_mb: parseInt(document.getElementById('job-memory').value, 10),
            max_retries: parseInt(document.getElementById('job-retries').value, 10),
            priority: parseInt(document.getElementById('job-priority').value, 10),
            use_custom_venv: document.getElementById('job-custom-venv').checked,
        };
        
        // Format dependencies as array of strings
        if (this.dependencies.length > 0) {
            jobData.dependencies = this.dependencies.map(d => 
                d.version ? `${d.name}${d.version}` : d.name
            );
        }
        
        // Validate
        if (!jobData.name) {
            Toast.error('Validation Error', 'Job name is required');
            document.getElementById('job-name').focus();
            return;
        }
        
        if (!jobData.python_code) {
            Toast.error('Validation Error', 'Python code is required');
            document.getElementById('job-code').focus();
            return;
        }
        
        if (jobData.timeout_seconds < 1 || jobData.timeout_seconds > 3600) {
            Toast.error('Validation Error', 'Timeout must be between 1 and 3600 seconds');
            return;
        }
        
        if (jobData.memory_limit_mb < 16 || jobData.memory_limit_mb > 4096) {
            Toast.error('Validation Error', 'Memory limit must be between 16 and 4096 MB');
            return;
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
        }
    },
};

// Initialize on DOM ready
document.addEventListener('DOMContentLoaded', () => {
    JobForm.init();
});
