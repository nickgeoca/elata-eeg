# EEG Data Exploration Toolkit

An open-source Python notebook for analyzing EEG datasets (e.g., OpenNeuro) with dimensionality reduction (t-SNE).

Notebook: openneuro.ipynb
Status: Research prototype (not a medical device).
### Project Description

This repository contains a Jupyter Notebook (openneuro.ipynb) for:

    Loading and preprocessing EEG data from public datasets (e.g., OpenNeuro).

    Calculating power spectral density (PSD) features and residualizing confounders (age/gender).

    Visualizing EEG patterns using t-SNE (a dimensionality reduction technique).

### ⚠️ Important Note:

    This is a research/educational tool only.

    It is not FDA-cleared and cannot diagnose, treat, or predict any medical condition (e.g., Alzheimer’s, dementia, or other diseases).

### Intended Use

    For neuroscience researchers, hobbyists, and developers exploring EEG signal processing.

    To reproduce experimental results from public datasets.

    As a template for open-data analysis (modify/extend for your needs).

### 🚫 What This Project Is NOT

    ❌ A medical device or diagnostic tool.

    ❌ Validated for clinical or health-related decisions.

    ❌ Intended to replace professional medical evaluation.

### How It Works

    Input Data: Uses EEG recordings (e.g., from OpenNeuro).

    Preprocessing: Filters, epochs, and extracts PSD features.

    Confounder Removal: Residualizes age/gender effects via linear regression.

    Visualization: Projects PSD features into 2D space using t-SNE.

Example output:
t-SNE plot of residualized PSD features
(Hypothetical clusters shown for illustration; no clinical interpretation implied.)

### ⚠️ Legal & Ethical Disclaimer

    This tool is provided "as-is" for non-medical use.

    By using this code, you agree:

        Not to rely on it for health-related decisions.

        To consult a qualified physician for medical concerns.

        To comply with applicable laws (e.g., FDA/FTC regulations if distributing derived products).

### 📚 Resources

    Datasets: OpenNeuro

    EEG Tools: MNE-Python

    Regulatory Info: FDA Digital Health Guidelines


> This tool helps explore EEG data patterns—but it is not a substitute for professional medical advice.


### 2-D Data Transform Tests
1) EEG Data -> 30sec segmentation -> Power Spectrum Density -> Residualize Confounds -> UMAP
  - seems to work
2) EEG Data -> 30sec segmentation -> Wave2Vec -> Residualize Confounds
  - Doesn't do as well (t-sne looks like noise)