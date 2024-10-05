
#!/bin/bash
if [ $# -eq 0 ]; then
    echo "pass version e.g. $(pwd)/$(basename "$0") 0.2.0"
 exit 1
fi


docker build -t hub.local.barnettjones.com/gem-mon:$1 .
