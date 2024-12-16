# How to do a release.

Go to src/fastled/__init__.py and increase the version number.

Make sure this is the ONLY change in your repo (or the release will fail
for unknown reasons) and commit and then push. Github builders will do all the rest.

Make sure and watch the jobs to verify that it worked.