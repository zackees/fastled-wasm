
# The only reason we use docker is because of the fastled-wasm-compiler.
FROM niteris/fastled-wasm-compiler:latest


COPY requirements.docker.txt /install/requirements.docker.txt

# Fresh packages need multiple hits to bust through stale cache... and this is fast.
RUN uv pip install --system -r /install/requirements.docker.txt --refresh  || \
    uv pip install --system -r /install/requirements.docker.txt --refresh || \
    uv pip install --system -r /install/requirements.docker.txt --refresh

# FIRST PRE-WARM CYCLE and initial setup: Download the fastled repo from the github and pre-warm the cache with a compilation.
# This is by far the most expensive part of the build, because platformio needs to download initial tools. This
# pre-warm cycle is "sticky" and tends to stay in the cache for a long time since docker is very relaxed about
# invalidating cache layers that clone a github repo.

ARG FASTLED_BUILD_DAY=echo $(date +'%Y-%m-%d')
ENV FASTLED_BUILD_DAY=${FASTLED_BUILD_DAY}


WORKDIR /js


RUN mkdir -p /js/compiler


# COPY compiler/*.py /js/compiler
COPY entrypoint.sh /entrypoint.sh
COPY compiler/run.py /js/run.py
COPY compiler/debug.sh /js/debug.sh

# COPY compiler/entrypoint.sh /entrypoint.sh
# RUN chmod +x /entrypoint.sh && dos2unix /entrypoint.sh

RUN cd /js && chmod +x debug.sh && dos2unix *.sh
RUN chmod +x /entrypoint.sh && dos2unix /entrypoint.sh

# RSYNC DISABLED FOR NOW
# now sync local to the source directory.
# RUN rsync -av /host/fastled/ /js/fastled/ && rm -rf /host/fastled

# RUN python /js/init_runtime.py || true


# SECOND PRE-WARM CYCLE: Copy the fastled repo from the host machine and pre-warm the cache with that compilation. This will
# be much quicker than the first pre-warm cycle.
# RUN /js/prewarm.sh --force > /logs/prewarm.log.2 || true

# Now timestamp the image and store it at the end of the build.
RUN date > /image_timestamp.txt

ENTRYPOINT ["/entrypoint.sh"]
CMD ["python", "/js/run.py", "server"]

# CMD ["/bin/bash", "/entrypoint.sh"]
