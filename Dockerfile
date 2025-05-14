
# The only reason we use docker is because of the fastled-wasm-compiler.
FROM niteris/fastled-wasm-compiler:latest


# Get the compiler requirements and install them.
COPY compiler/pyproject.toml /install/pyproject.toml
RUN uv pip install --system -r /install/pyproject.toml

# FIRST PRE-WARM CYCLE and initial setup: Download the fastled repo from the github and pre-warm the cache with a compilation.
# This is by far the most expensive part of the build, because platformio needs to download initial tools. This
# pre-warm cycle is "sticky" and tends to stay in the cache for a long time since docker is very relaxed about
# invalidating cache layers that clone a github repo.

RUN echo "force update4"
ARG FASTLED_BUILD_DAY=echo $(date +'%Y-%m-%d')
ENV FASTLED_BUILD_DAY=${FASTLED_BUILD_DAY}

# ARG FASTLED_VERSION=master
# ENV FASTLED_VERSION=${FASTLED_VERSION}
# RUN mkdir -p /js/fastled && \
#     rsync -a /git/fastled/ /js/fastled/ --exclude='.git'

# Create symlinks for wasm platform files.
COPY compiler/init_runtime.py /js/init_runtime.py
#COPY compiler/prewarm.sh /js/prewarm.sh

WORKDIR /js

#ARG NO_PREWARM=0
#ENV NO_PREWARM=${NO_PREWARM}

#RUN python /js/init_runtime.py || true


# First pre-warm cycle - always do it as part of the build.
# RUN mkdir -p /logs

# Force a build if the compiler flags change.
#COPY compiler/CMakeLists.txt /trash/CMakeLists.txt
#RUN rm -rf /trash

#RUN chmod +x /js/prewarm.sh && \
#    cat /js/prewarm.sh >> /logs/prewarm.log.0
#RUN /js/prewarm.sh --force >> /logs/prewarm.log.1 || true



# # Copy the fastled repo from the host machine and prepare for pre-warm
# # Make sure and delete files that have been removed so that we don't get
# # duplicate symbols from stale files.
# COPY *.json /host/fastled/
# COPY src/*.* /host/fastled/src/
# COPY examples /host/fastled/examples
# COPY src/fx /host/fastled/src/fx
# COPY src/fl /host/fastled/src/fl
# COPY src/lib8tion /host/fastled/src/lib8tion
# COPY src/third_party /host/fastled/src/third_party
# COPY src/sensors /host/fastled/src/sensors
# COPY src/platforms /host/fastled/src/platforms

RUN echo "force update"
COPY compiler /js/compiler

COPY compiler/entrypoint.sh /entrypoint.sh
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
