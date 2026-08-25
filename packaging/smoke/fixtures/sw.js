self.addEventListener('message', function (event) {
  if (event.data === 'off-list-fetch') {
    fetch('http://evil.test/service-worker').catch(function () {});
  }
});
