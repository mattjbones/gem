import fs from 'node:fs';

// Takes the env file and returns a string for docker-compose

fs.readFile('./.env', 'utf8', (err, data) => {
  if (err) {
    console.error(err);
    return;
  }
 console.log(JSON.stringify(data.split("\n").map(line => !line.startsWith("#") ? `${line}` : undefined).filter(Boolean)));
});
